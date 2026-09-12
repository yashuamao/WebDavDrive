//! foundation-secrets：密钥存储抽象与密钥生命周期守卫。
//!
//! 背景（旧版实测）：
//! - Windows 上用**机器范围** DPAPI：boot/SYSTEM 模式需要无交互解密；
//!   代价是同机任意用户都能解密，因此数据目录 ACL 必须另行收紧（见 foundation-windows）。
//! - 密钥丢失/损坏时**必须拒绝启动**，绝不静默重建，否则旧密文永久无法解开。
//! - 解密失败必须显式报错，调用方不得用空值覆盖既有凭据。
//!
//! 本 crate 不知道密钥用来保护什么，只负责"生成/存取/守卫"。

mod file;
mod hex;
mod keyring;

#[cfg(windows)]
mod dpapi;

pub use file::FileSecretStore;
pub use keyring::{KeyRing, KeyState};

/// 可插拔的密钥保护后端。
pub trait SecretStore: Send + Sync + 'static {
    /// 后端名称（日志用），如 "dpapi-machine" / "plain-file"。
    fn name(&self) -> &'static str;
    /// 把明文变成不透明令牌（可安全落盘）。
    fn protect(&self, plaintext: &str) -> foundation_core::Result<String>;
    /// 从令牌还原明文；失败必须返回错误。
    fn unprotect(&self, token: &str) -> foundation_core::Result<String>;
}

/// 根据平台选择默认后端：
/// - Windows → 机器范围 DPAPI；
/// - 其他平台 → 受限权限文件（明文，仅供开发/测试；NAS 正式部署应注入系统密钥源）。
pub fn default_store(plain_file_path: std::path::PathBuf) -> Box<dyn SecretStore> {
    #[cfg(windows)]
    {
        let _ = plain_file_path; // Windows 不使用明文文件
        Box::new(dpapi::DpapiMachineStore::new())
    }
    #[cfg(not(windows))]
    {
        Box::new(FileSecretStore::new(plain_file_path))
    }
}
