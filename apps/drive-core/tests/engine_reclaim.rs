#![cfg(windows)]
//! AC-28 真机验收：宿主被强杀（TerminateProcess）后，Job Object 必须回收 rclone。
//!
//! 用 `drive-engine-holder` 作为宿主子进程：它启动真实引擎、写出 rclone pid，然后等待被强杀。
//! 测试强杀它之后轮询 rclone 是否消失。没有 rclone 时跳过。

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn pid_alive(pid: u32) -> bool {
    let Ok(output) = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
    else {
        return false;
    };
    String::from_utf8_lossy(&output.stdout).contains(&pid.to_string())
}

#[test]
fn engine_is_reclaimed_when_host_is_hard_killed() {
    let Some(rclone) = find_rclone() else {
        eprintln!("SKIP: 找不到 rclone.exe");
        return;
    };

    let dir = std::env::temp_dir().join(format!("drive-reclaim-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let ready_file = dir.join("engine.pid");

    let mut holder = Command::new(env!("CARGO_BIN_EXE_drive-engine-holder"))
        .arg(&ready_file)
        .arg(&rclone)
        .arg(dir.join("data"))
        .arg(env!("CARGO_BIN_EXE_drive-pwcmd"))
        .arg(free_port().to_string())
        .stdin(Stdio::null())
        .spawn()
        .expect("无法启动 engine_holder");

    // 等待引擎就绪 + pid 落盘
    let deadline = Instant::now() + Duration::from_secs(60);
    let engine_pid: u32 = loop {
        if let Some(status) = holder.try_wait().unwrap() {
            panic!("holder 提前退出：{status}");
        }
        if let Ok(text) = std::fs::read_to_string(&ready_file) {
            if let Ok(pid) = text.trim().parse() {
                break pid;
            }
        }
        if Instant::now() > deadline {
            let _ = holder.kill();
            panic!("等待引擎就绪超时");
        }
        std::thread::sleep(Duration::from_millis(300));
    };

    assert!(
        pid_alive(engine_pid),
        "holder 报告引擎 pid {engine_pid}，但进程不存在"
    );

    // 强杀宿主（TerminateProcess），等价于任务管理器结束进程 / 崩溃
    holder.kill().expect("强杀 holder 失败");
    let _ = holder.wait();

    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline && pid_alive(engine_pid) {
        std::thread::sleep(Duration::from_millis(300));
    }

    let reclaimed = !pid_alive(engine_pid);
    if !reclaimed {
        // 清理可能残留的引擎，避免污染后续测试
        let _ = Command::new("taskkill")
            .args(["/F", "/PID", &engine_pid.to_string()])
            .output();
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        reclaimed,
        "宿主被强杀后 rclone（pid {engine_pid}）仍存活：Job Object 未生效"
    );
}
