use std::ptr;

use foundation_core::{FoundationError, Result};
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};

use crate::{hex, SecretStore};

// crypt32.h：这两个常量没有随 windows-sys 导出，自己定义更稳。
const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x01;
const CRYPTPROTECT_LOCAL_MACHINE: u32 = 0x04;
const FLAGS: u32 = CRYPTPROTECT_UI_FORBIDDEN | CRYPTPROTECT_LOCAL_MACHINE;

/// Windows 机器范围 DPAPI。
///
/// 语义说明（必须如实告知用户）：机器范围意味着**同机任意用户**都能解密，
/// 不只是管理员。因此密钥文件所在目录必须配合 ACL 收紧（foundation-windows::harden_data_dir）。
#[derive(Debug, Default)]
pub struct DpapiMachineStore;

impl DpapiMachineStore {
    pub fn new() -> Self {
        Self
    }
}

fn last_error() -> String {
    std::io::Error::last_os_error().to_string()
}

fn protect_bytes(data: &[u8]) -> Result<Vec<u8>> {
    let mut input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    let ok = unsafe {
        CryptProtectData(
            &input,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            FLAGS,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(FoundationError::SecretDecrypt(format!(
            "CryptProtectData 失败：{}",
            last_error()
        )));
    }
    let protected =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as _);
    }
    Ok(protected)
}

fn unprotect_bytes(data: &[u8]) -> Result<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(FoundationError::SecretDecrypt(format!(
            "CryptUnprotectData 失败：{}",
            last_error()
        )));
    }
    let plain =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as _);
    }
    Ok(plain)
}

impl SecretStore for DpapiMachineStore {
    fn name(&self) -> &'static str {
        "dpapi-machine"
    }

    fn protect(&self, plaintext: &str) -> Result<String> {
        protect_bytes(plaintext.as_bytes()).map(|bytes| hex::encode(&bytes))
    }

    fn unprotect(&self, token: &str) -> Result<String> {
        let bytes = hex::decode(token)
            .ok_or_else(|| FoundationError::SecretDecrypt("令牌不是合法 hex".into()))?;
        let plain = unprotect_bytes(&bytes)?;
        String::from_utf8(plain)
            .map_err(|err| FoundationError::SecretDecrypt(format!("明文不是 UTF-8：{err}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpapi_roundtrip_and_random_salt() {
        let store = DpapiMachineStore::new();
        let secret = "p@ssw0rd-汉字";
        let token = store.protect(secret).unwrap();
        assert_ne!(token, secret, "密文不应等于明文");
        assert_eq!(store.unprotect(&token).unwrap(), secret);
        assert_ne!(
            store.protect(secret).unwrap(),
            token,
            "DPAPI 含随机盐，两次密文应不同"
        );
    }

    #[test]
    fn dpapi_rejects_garbage_token() {
        let store = DpapiMachineStore::new();
        assert!(store.unprotect("not-a-token").is_err());
        let bad_hex = hex::encode(b"definitely-not-a-dpapi-blob");
        assert!(store.unprotect(&bad_hex).is_err());
    }
}
