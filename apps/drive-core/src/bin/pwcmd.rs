//! `rclone --password-command` 目标程序：把配置加密口令打到 stdout。
//!
//! 用法：`drive-pwcmd --key-file <config_key.enc>`
//! 密钥文件是 SecretStore 保护的令牌；Windows 用机器范围 DPAPI 解密。
//! 失败时明确返回非零并写 stderr，绝不输出空口令（rclone 会拿到空值产生误导错误）。

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut key_file: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--key-file" {
            key_file = args.next();
        }
    }
    let Some(key_file) = key_file else {
        eprintln!("pwcmd: 缺少 --key-file 参数");
        return ExitCode::from(2);
    };

    let token = match std::fs::read_to_string(&key_file) {
        Ok(text) => text.trim().to_string(),
        Err(err) => {
            eprintln!("pwcmd: 无法读取密钥文件 {key_file}：{err}");
            return ExitCode::from(1);
        }
    };
    if token.is_empty() {
        eprintln!("pwcmd: 密钥文件 {key_file} 为空");
        return ExitCode::from(1);
    }

    #[cfg(windows)]
    {
        use foundation_secrets::{DpapiMachineStore, SecretStore};
        match DpapiMachineStore::new().unprotect(&token) {
            Ok(secret) => {
                print!("{secret}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("pwcmd: 无法解密 {key_file}：{err}");
                ExitCode::from(1)
            }
        }
    }
    #[cfg(not(windows))]
    {
        // 非 Windows 的文件后端令牌即明文（仅开发环境）
        print!("{token}");
        ExitCode::SUCCESS
    }
}
