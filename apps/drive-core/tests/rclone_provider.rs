//! 真 rclone 集成测试（不需要 WinFsp，也不挂载）：
//! AC-23/34/35/36/37 + 密钥生命周期 AC-26。
//!
//! 引擎路径：RCLONE_EXE 环境变量 → PATH → 本机旧仓库 bin（开发机便利）。
//! 找不到时跳过并打印 SKIP，不让 CI 因环境缺引擎而红。

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use drive_core::model::{Connection, ConnectionInput};
use drive_core::provider::{EngineConfig, MountProvider, RcloneProvider};
use foundation_secrets::{default_store, SecretStore};

fn find_rclone() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("RCLONE_EXE") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join("rclone.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let legacy = PathBuf::from(r"Z:\AI\webdav-drive\bin\rclone.exe");
    legacy.is_file().then_some(legacy)
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("drive-rclone-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn make_provider(dir: &PathBuf, rclone: PathBuf) -> (RcloneProvider, Arc<dyn SecretStore>) {
    let secrets = default_store(dir.join("keystore.bin"));
    let mut config = EngineConfig::new(dir, format!("127.0.0.1:{}", free_port()));
    config.rclone_path = Some(rclone);
    config.ready_timeout = Duration::from_secs(20);
    let key_path = config.key_path();
    config.password_command = format!(
        "\"{}\" --key-file \"{}\"",
        env!("CARGO_BIN_EXE_drive-pwcmd"),
        key_path.display()
    );
    (RcloneProvider::new(config, secrets.clone()), secrets)
}

fn make_connection(secrets: &dyn SecretStore) -> Connection {
    Connection::from_input(
        ConnectionInput {
            name: Some("集成测试".into()),
            url: Some("http://127.0.0.1:1/dav".into()),
            user: Some("tester".into()),
            password: Some("pw-汉字".into()),
            ..Default::default()
        },
        None,
        secrets,
    )
    .unwrap()
}

#[test]
fn engine_starts_encrypts_config_and_creates_remote() {
    let Some(rclone) = find_rclone() else {
        eprintln!("SKIP: 找不到 rclone.exe");
        return;
    };
    let dir = temp_dir("engine");
    let (provider, secrets) = make_provider(&dir, rclone);
    let connection = make_connection(secrets.as_ref());

    provider.ensure_started().expect("引擎应能启动");
    let status = provider.engine_status();
    assert!(status.installed && status.running, "{status:?}");
    assert!(
        status.version.as_deref().unwrap_or("").starts_with('v'),
        "应读到 rclone 版本：{status:?}"
    );

    // AC-37：remote 名与密码不得出现在配置文件明文里
    provider
        .ensure_remote(&connection, "pw-汉字")
        .expect("remote 应创建成功");
    let conf = std::fs::read_to_string(dir.join("rclone.conf")).unwrap();
    assert!(
        conf.starts_with("# Encrypted rclone configuration File"),
        "配置必须静态加密：{}",
        &conf[..conf.len().min(80)]
    );
    assert!(
        !conf.contains(&connection.remote),
        "remote 名不得出现在明文配置"
    );
    assert!(!conf.contains("pw-汉字"), "密码不得出现在明文配置");
    assert!(dir.join("config_key.enc").exists(), "密钥文件应生成");

    // AC-34：list 可用（没有挂载也应是空列表而不是错误）
    assert!(provider.list().unwrap().is_empty());

    // AC-23：RC 拒绝未认证请求（口令走环境变量注入，确实生效）
    assert_eq!(unauthorized_rc_status(&provider.config().rc_addr), 401);

    // AC-36：探测不可达地址应返回明确错误，而不是伪装成功
    let probe_err = provider.probe(&connection, "pw-汉字").unwrap_err();
    assert_eq!(probe_err.code(), "process", "{probe_err}");

    // AC-35：挂载失败（本机没有 WinFsp）必须干净报错，且不拖死引擎
    let mount_err = provider.mount(&connection, "pw-汉字").unwrap_err();
    assert_eq!(mount_err.code(), "process", "{mount_err}");
    assert!(provider.engine_status().running, "挂载失败后引擎仍应在运行");
    assert!(provider.list().is_ok(), "挂载失败后 RC 仍应可用");

    provider.shutdown().unwrap();
    assert!(!provider.engine_status().running, "shutdown 后不应再运行");
}

#[test]
fn encrypted_config_without_key_refuses_to_start() {
    let Some(rclone) = find_rclone() else {
        eprintln!("SKIP: 找不到 rclone.exe");
        return;
    };
    let dir = temp_dir("lost-key");
    let (provider, _secrets) = make_provider(&dir, rclone);
    provider.ensure_started().expect("首次启动应成功");
    provider.shutdown().unwrap();

    // 模拟用户只删了密钥、留下加密配置（AC-26）
    std::fs::remove_file(dir.join("config_key.enc")).unwrap();

    let (provider2, _) = make_provider(&dir, provider.config().rclone_path.clone().unwrap());
    let err = provider2.ensure_started().unwrap_err();
    assert_eq!(err.code(), "key_unavailable", "{err}");
}

/// 不带 Authorization 直接请求 RC，读取 HTTP 状态码。
fn unauthorized_rc_status(addr: &str) -> u16 {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    let request = format!(
        "POST /core/version HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
    );
    let _ = stream.write_all(request.as_bytes());
    let mut buf = [0u8; 64];
    let read = stream.read(&mut buf).unwrap_or(0);
    let head = String::from_utf8_lossy(&buf[..read]);
    head.split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0)
}
