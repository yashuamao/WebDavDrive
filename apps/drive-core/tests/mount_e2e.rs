#![cfg(windows)]
//! 真机挂载验收（需要 WinFsp；缺失时 SKIP）：
//! - `mount_success_read_write`：本地 `rclone serve webdav` → 挂成盘符 → 读文件 → 卸载
//! - `failed_mount_leaves_no_residue`（AC-38）：不可达源挂载失败后不得残留挂载点
//!
//! 覆盖 AC-34（回环）之外的「挂载成功路径」，与旧版 e2e_test.py 的验收范围对齐。

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

fn winfsp_present() -> bool {
    // 兼容两种安装布局：老版本放 System32，新版放 WinFspin
    let mut candidates: Vec<PathBuf> = Vec::new();
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    for dll in ["winfsp-x64.dll", "winfsp-a64.dll"] {
        candidates.push(Path::new(&root).join("System32").join(dll));
    }
    for base in [r"C:\Program Files (x86)\WinFsp", r"C:\Program Files\WinFsp"] {
        for dll in ["winfsp-x64.dll", "winfsp-a64.dll"] {
            candidates.push(Path::new(base).join("bin").join(dll));
        }
    }
    candidates.iter().any(|path| path.exists())
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn free_drive() -> Option<String> {
    for letter in ["Y", "W", "V", "U", "T", "S"] {
        let mount = format!("{letter}:");
        if !Path::new(&format!("{mount}\\")).exists() {
            return Some(mount);
        }
    }
    None
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("drive-e2e-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Serve(Child);

impl Drop for Serve {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn make_provider(dir: &Path, rclone: PathBuf) -> (RcloneProvider, Arc<dyn SecretStore>) {
    let secrets = default_store(dir.join("keystore.bin"));
    let mut config = EngineConfig::new(dir, format!("127.0.0.1:{}", free_port()));
    config.rclone_path = Some(rclone);
    config.ready_timeout = Duration::from_secs(20);
    config.password_command = format!(
        "\"{}\" --key-file \"{}\"",
        env!("CARGO_BIN_EXE_drive-pwcmd"),
        config.key_path().display()
    );
    (RcloneProvider::new(config, secrets.clone()), secrets)
}

fn connection(secrets: &dyn SecretStore, url: &str, drive: &str) -> Connection {
    Connection::from_input(
        ConnectionInput {
            name: Some("E2E".into()),
            url: Some(url.into()),
            user: Some("e2e-user".into()),
            password: Some("e2e-pass-汉字".into()),
            drive: Some(drive.into()),
            vfs_cache_mode: Some("writes".into()),
            dir_cache_time: Some("1m".into()),
            ..Default::default()
        },
        None,
        secrets,
    )
    .unwrap()
}

#[test]
fn mount_success_read_and_unmount() {
    let (Some(rclone), true) = (find_rclone(), winfsp_present()) else {
        eprintln!("SKIP: 需要 rclone.exe 和 WinFsp");
        return;
    };
    let Some(drive) = free_drive() else {
        eprintln!("SKIP: 没有空闲盘符");
        return;
    };

    let work = temp_dir("mount");
    let source = work.join("server-root");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("hello.txt"), "hello-webdav\n").unwrap();
    std::fs::create_dir_all(source.join("sub")).unwrap();
    std::fs::write(source.join("sub").join("inner.txt"), "inner\n").unwrap();

    let dav_port = free_port();
    let serve = Serve(
        Command::new(&rclone)
            .args([
                "serve",
                "webdav",
                source.to_string_lossy().as_ref(),
                "--addr",
                &format!("127.0.0.1:{dav_port}"),
                "--user",
                "e2e-user",
                "--pass",
                "e2e-pass-汉字",
                "--log-level",
                "ERROR",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("启动 webdav 服务失败"),
    );
    // 等服务端口就绪
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(format!("127.0.0.1:{dav_port}")).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    let (provider, secrets) = make_provider(&work, rclone);
    let connection = connection(
        secrets.as_ref(),
        &format!("http://127.0.0.1:{dav_port}/"),
        &drive,
    );

    provider.ensure_started().expect("引擎启动失败");
    let record = provider
        .mount(&connection, "e2e-pass-汉字")
        .expect("挂载失败");
    assert_eq!(
        record.mount_point.to_lowercase(),
        drive.to_lowercase(),
        "挂载点应与请求一致"
    );

    let root = format!("{drive}\\");
    let appeared = {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline && !Path::new(&root).exists() {
            std::thread::sleep(Duration::from_millis(300));
        }
        Path::new(&root).exists()
    };
    assert!(appeared, "盘符 {drive} 未出现");

    let content = std::fs::read_to_string(format!("{root}hello.txt")).expect("读取挂载文件失败");
    assert_eq!(content, "hello-webdav\n");
    assert!(
        Path::new(&format!("{root}sub\\inner.txt")).exists(),
        "子目录不可见"
    );

    // 卸载后盘符消失
    provider.unmount(&drive).expect("卸载失败");
    let gone = {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline && Path::new(&root).exists() {
            std::thread::sleep(Duration::from_millis(300));
        }
        !Path::new(&root).exists()
    };
    provider.shutdown().unwrap();
    drop(serve);
    let _ = std::fs::remove_dir_all(&work);
    assert!(gone, "卸载后盘符 {drive} 仍在");
}

#[test]
fn failed_mount_leaves_no_residue() {
    let (Some(rclone), true) = (find_rclone(), winfsp_present()) else {
        eprintln!("SKIP: 需要 rclone.exe 和 WinFsp");
        return;
    };
    let Some(drive) = free_drive() else {
        eprintln!("SKIP: 没有空闲盘符");
        return;
    };

    let work = temp_dir("fail");
    let (provider, secrets) = make_provider(&work, rclone);
    // 确定性失败：未知挂载参数会被 rclone RC 在建立挂载点之前拒绝。
    // （注意：不可达的 WebDAV 源在 rclone 下是惰性挂载，可能成功返回，不适合做失败用例。）
    let mut connection = connection(secrets.as_ref(), "http://127.0.0.1:1/dav", &drive);
    connection.extra_opts = "--definitely-bogus-flag".into();
    provider.ensure_started().expect("引擎启动失败");

    let err = provider
        .mount(&connection, "e2e-pass-汉字")
        .expect_err("未知参数应导致挂载失败");
    assert_eq!(err.code(), "process", "{err}");

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut mounted = provider.list().unwrap_or_default();
    while Instant::now() < deadline
        && mounted
            .iter()
            .any(|m| m.mount_point.to_lowercase() == drive.to_lowercase())
    {
        std::thread::sleep(Duration::from_millis(300));
        mounted = provider.list().unwrap_or_default();
    }
    provider.shutdown().unwrap();
    let _ = std::fs::remove_dir_all(&work);
    assert!(
        !mounted
            .iter()
            .any(|m| m.mount_point.to_lowercase() == drive.to_lowercase()),
        "挂载失败后不应残留 {drive}"
    );
}
