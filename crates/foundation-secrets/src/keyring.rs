use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use foundation_core::{FoundationError, Result};
use uuid::Uuid;

use crate::SecretStore;

/// `ensure_key` 的结果：密钥已存在（解密得到）或本次新建。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyState {
    Existing(String),
    Created(String),
}

impl KeyState {
    pub fn secret(&self) -> &str {
        match self {
            Self::Existing(s) | Self::Created(s) => s,
        }
    }
}

/// 配置加密密钥的生命周期守卫。
pub struct KeyRing {
    store: Arc<dyn SecretStore>,
    key_path: PathBuf,
}

impl KeyRing {
    pub fn new(store: Arc<dyn SecretStore>, key_path: impl Into<PathBuf>) -> Self {
        Self {
            store,
            key_path: key_path.into(),
        }
    }

    pub fn store_name(&self) -> &'static str {
        self.store.name()
    }

    pub fn key_path(&self) -> &Path {
        &self.key_path
    }

    /// 读取已有密钥；文件存在但损坏/解不开 → 明确报错（绝不返回 None 让调用方重建）。
    pub fn read_key(&self) -> Result<Option<String>> {
        if !self.key_path.exists() {
            return Ok(None);
        }
        let token = fs::read_to_string(&self.key_path)?;
        let token = token.trim();
        if token.is_empty() {
            return Err(FoundationError::KeyUnavailable(format!(
                "密钥文件 {} 为空",
                self.key_path.display()
            )));
        }
        self.store.unprotect(token).map(Some).map_err(|err| {
            FoundationError::KeyUnavailable(format!(
                "密钥文件 {} 无法解密（{err}）；若它被换过或损坏，请同时删除密钥与已加密配置后重建",
                self.key_path.display()
            ))
        })
    }

    /// 保证密钥可用。
    ///
    /// `payload_is_encrypted` 表示"已有用该密钥加密的配置存在"：
    /// 此时若密钥文件缺失，必须报错而不是新建——否则旧配置永远解不开，
    /// 而且错误会推迟到第一次读写配置时才暴露。
    pub fn ensure_key(&self, payload_is_encrypted: bool) -> Result<KeyState> {
        if let Some(existing) = self.read_key()? {
            return Ok(KeyState::Existing(existing));
        }
        if payload_is_encrypted {
            return Err(FoundationError::KeyUnavailable(format!(
                "配置已加密但密钥文件 {} 不存在；请删除已加密配置后重建",
                self.key_path.display()
            )));
        }

        // 两个 UUIDv4 拼起来：244 位随机，足够作为配置口令
        let secret = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let token = self.store.protect(&secret)?;

        if let Some(parent) = self.key_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = self.key_path.with_extension("enc.tmp");
        fs::write(&tmp, token.as_bytes())?;
        fs::rename(&tmp, &self.key_path)?;
        log::info!("已生成新的密钥：{}", self.key_path.display());
        Ok(KeyState::Created(secret))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FileSecretStore;

    fn dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("foundation-key-{tag}-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ring(tag: &str) -> (KeyRing, PathBuf) {
        let dir = dir(tag);
        let key_path = dir.join("config_key.enc");
        let store = Arc::new(FileSecretStore::new(dir.join("keystore.bin")));
        (KeyRing::new(store, key_path.clone()), key_path)
    }

    #[test]
    fn creates_then_reads_same_key() {
        let (ring, path) = ring("create");
        let created = ring.ensure_key(false).unwrap();
        assert!(matches!(created, KeyState::Created(_)));
        assert!(path.exists());

        let again = ring.ensure_key(false).unwrap();
        assert_eq!(again, KeyState::Existing(created.secret().to_string()));
    }

    #[test]
    fn missing_key_with_encrypted_payload_must_fail() {
        let (ring, _) = ring("missing");
        let err = ring.ensure_key(true).unwrap_err();
        assert_eq!(err.code(), "key_unavailable");
        assert!(err.to_string().contains("不存在"));
    }

    #[test]
    fn corrupt_key_file_must_fail_not_rebuild() {
        let (ring, path) = ring("corrupt");
        fs::write(&path, "garbage-token").unwrap();
        let err = ring.ensure_key(false).unwrap_err();
        assert_eq!(err.code(), "key_unavailable");
    }

    #[test]
    fn empty_key_file_must_fail() {
        let (ring, path) = ring("empty");
        fs::write(&path, "").unwrap();
        let err = ring.ensure_key(false).unwrap_err();
        assert_eq!(err.code(), "key_unavailable");
    }
}
