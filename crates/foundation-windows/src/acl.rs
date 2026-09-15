//! 数据目录 ACL 收紧。
//!
//! 为什么必须做：`%PROGRAMDATA%` 默认给 `BUILTIN\Users:(OI)(CI)(RX)`，而机器范围 DPAPI
//! 是"同机任意用户都能解密"。不收权等于把凭据摊给所有本地账户。
//!
//! 使用 Win32 ACL API 为目录树逐项替换 DACL；不能只移除继承并追加授权，因为既有的
//! 显式 Everyone/其他账户 ACE 会继续保留。目录拒绝重解析点，避免越界修改目标 ACL。

use std::path::Path;

#[cfg(not(windows))]
use foundation_core::FoundationError;
use foundation_core::Result;

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
    windows_acl::harden_tree(path)
}

#[cfg(windows)]
mod windows_acl {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::MetadataExt;
    use std::path::Path;
    use std::ptr::{null, null_mut};

    use foundation_core::{FoundationError, Result};
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, ERROR_SUCCESS, HANDLE};
    use windows_sys::Win32::Security::Authorization::{
        SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W, SET_ACCESS, SE_FILE_OBJECT,
        TRUSTEE_IS_SID, TRUSTEE_IS_UNKNOWN, TRUSTEE_W,
    };
    use windows_sys::Win32::Security::{
        CopySid, CreateWellKnownSid, GetLengthSid, GetTokenInformation, TokenUser,
        WinBuiltinAdministratorsSid, WinLocalSystemSid, ACL, DACL_SECURITY_INFORMATION,
        NO_INHERITANCE, PROTECTED_DACL_SECURITY_INFORMATION, SECURITY_MAX_SID_SIZE,
        SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_QUERY, TOKEN_USER, WELL_KNOWN_SID_TYPE,
    };
    use windows_sys::Win32::Storage::FileSystem::{FILE_ALL_ACCESS, FILE_ATTRIBUTE_REPARSE_POINT};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    struct OwnedHandle(HANDLE);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    struct OwnedAcl(*mut ACL);

    impl Drop for OwnedAcl {
        fn drop(&mut self) {
            unsafe { LocalFree(self.0.cast()) };
        }
    }

    struct AllowedSids {
        system: Vec<u8>,
        administrators: Vec<u8>,
        current_user: Vec<u8>,
    }

    pub fn harden_tree(path: &Path) -> Result<()> {
        let allowed = AllowedSids {
            system: well_known_sid(WinLocalSystemSid)?,
            administrators: well_known_sid(WinBuiltinAdministratorsSid)?,
            current_user: current_user_sid()?,
        };
        harden_entry(path, true, &allowed)?;
        harden_children(path, &allowed)
    }

    fn harden_children(directory: &Path, allowed: &AllowedSids) -> Result<()> {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(FoundationError::Platform(format!(
                    "数据目录包含符号链接或重解析点，拒绝跨边界修改 ACL：{}",
                    path.display()
                )));
            }
            let is_dir = metadata.is_dir();
            harden_entry(&path, is_dir, allowed)?;
            if is_dir {
                harden_children(&path, allowed)?;
            }
        }
        Ok(())
    }

    fn harden_entry(path: &Path, is_dir: bool, allowed: &AllowedSids) -> Result<()> {
        let inheritance = if is_dir {
            SUB_CONTAINERS_AND_OBJECTS_INHERIT
        } else {
            NO_INHERITANCE
        };
        let entries = [
            explicit_access(&allowed.system, inheritance),
            explicit_access(&allowed.administrators, inheritance),
            explicit_access(&allowed.current_user, inheritance),
        ];
        let mut acl: *mut ACL = null_mut();
        let status =
            unsafe { SetEntriesInAclW(entries.len() as u32, entries.as_ptr(), null(), &mut acl) };
        if status != ERROR_SUCCESS {
            return Err(win32_status("SetEntriesInAclW", status));
        }
        let acl = OwnedAcl(acl);
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let status = unsafe {
            SetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                acl.0,
                null(),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(win32_status(
                &format!("SetNamedSecurityInfoW({})", path.display()),
                status,
            ));
        }
        Ok(())
    }

    fn explicit_access(sid: &[u8], inheritance: u32) -> EXPLICIT_ACCESS_W {
        EXPLICIT_ACCESS_W {
            grfAccessPermissions: FILE_ALL_ACCESS,
            grfAccessMode: SET_ACCESS,
            grfInheritance: inheritance,
            Trustee: TRUSTEE_W {
                pMultipleTrustee: null_mut(),
                MultipleTrusteeOperation: 0,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_UNKNOWN,
                ptstrName: sid.as_ptr() as *mut u16,
            },
        }
    }

    fn well_known_sid(kind: WELL_KNOWN_SID_TYPE) -> Result<Vec<u8>> {
        let mut sid = vec![0u8; SECURITY_MAX_SID_SIZE as usize];
        let mut size = sid.len() as u32;
        let ok =
            unsafe { CreateWellKnownSid(kind, null_mut(), sid.as_mut_ptr().cast(), &mut size) };
        if ok == 0 {
            return Err(last_error("CreateWellKnownSid"));
        }
        sid.truncate(size as usize);
        Ok(sid)
    }

    fn current_user_sid() -> Result<Vec<u8>> {
        let mut token: HANDLE = null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(last_error("OpenProcessToken"));
        }
        let token = OwnedHandle(token);
        let mut size = 0u32;
        unsafe {
            GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut size);
        }
        if size == 0 {
            return Err(last_error("GetTokenInformation(size)"));
        }
        let mut buffer = vec![0u8; size as usize];
        if unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                size,
                &mut size,
            )
        } == 0
        {
            return Err(last_error("GetTokenInformation(TokenUser)"));
        }
        let token_user = unsafe { &*(buffer.as_ptr() as *const TOKEN_USER) };
        let sid_len = unsafe { GetLengthSid(token_user.User.Sid) };
        if sid_len == 0 {
            return Err(last_error("GetLengthSid"));
        }
        let mut sid = vec![0u8; sid_len as usize];
        if unsafe { CopySid(sid_len, sid.as_mut_ptr().cast(), token_user.User.Sid) } == 0 {
            return Err(last_error("CopySid"));
        }
        Ok(sid)
    }

    fn last_error(context: &str) -> FoundationError {
        FoundationError::Platform(format!(
            "{context} 失败：{}",
            std::io::Error::last_os_error()
        ))
    }

    fn win32_status(context: &str, status: u32) -> FoundationError {
        FoundationError::Platform(format!(
            "{context} 失败：{}",
            std::io::Error::from_raw_os_error(status as i32)
        ))
    }
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
        let granted = std::process::Command::new("icacls")
            .arg(&file)
            .args(["/grant", "*S-1-1-0:(R)"])
            .status()
            .unwrap();
        assert!(granted.success(), "测试前应能显式授予 Everyone");

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
        assert!(
            !acl_text.contains("Everyone") && !acl_text.contains("S-1-1-0"),
            "收紧后不应保留显式 Everyone ACE：{acl_text}"
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
