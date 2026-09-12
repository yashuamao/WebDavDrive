use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use foundation_core::{FoundationError, Result};

use crate::SecretStore;

/// 受限权限文件后端（明文）。
///
/// 明确说明：**这不是加密**，只是"把密钥放在只有本用户可读的文件里"。
/// Windows 生产路径必须用 DPAPI；本后端用于测试与非 Windows 平台的开发环境。
#[derive(Debug, Clone)]
pub struct FileSecretStore {
    path: PathBuf,
}

impl FileSecretStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn write_restricted(&self, content: &str) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(&self.path)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        restrict_permissions(&self.path)?;
        Ok(())
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<()> {
    // Windows 上没有 POSIX 权限位；生产路径请使用 DPAPI。
    Ok(())
}

impl SecretStore for FileSecretStore {
    fn name(&self) -> &'static str {
        "plain-file"
    }

    fn protect(&self, plaintext: &str) -> Result<String> {
        self.write_restricted(plaintext)?;
        Ok(plaintext.to_string())
    }

    fn unprotect(&self, token: &str) -> Result<String> {
        let stored = fs::read_to_string(&self.path)?;
        let stored = stored.trim_end_matches(['\r', '\n']);
        if stored != token.trim_end_matches(['\r', '\n']) {
            return Err(FoundationError::SecretDecrypt(format!(
                "{} 的内容与传入令牌不一致（密钥文件可能已被替换）",
                self.path.display()
            )));
        }
        Ok(stored.to_string())
    }
}
