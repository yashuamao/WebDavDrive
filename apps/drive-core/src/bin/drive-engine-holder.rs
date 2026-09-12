//! 测试辅助工具（不随发布包分发）：启动真实 rclone 引擎并写出其 pid，然后等待被强杀。
//!
//! 用于验证 AC-28：宿主进程被 TerminateProcess 后，Job Object 回收 rclone。
//! 用法：drive-engine-holder <ready-file> <rclone.exe> <data-dir> <pwcmd.exe> <rc-port>

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use drive_core::provider::{EngineConfig, MountProvider, RcloneProvider};
use foundation_secrets::default_store;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 6 {
        eprintln!(
            "usage: drive-engine-holder <ready-file> <rclone.exe> <data-dir> <pwcmd.exe> <rc-port>"
        );
        std::process::exit(2);
    }
    let ready_file = PathBuf::from(&args[1]);
    let rclone = PathBuf::from(&args[2]);
    let data_dir = PathBuf::from(&args[3]);
    let pwcmd = PathBuf::from(&args[4]);
    let rc_port: u16 = args[5].parse().expect("rc-port 必须是数字");
    std::fs::create_dir_all(&data_dir).expect("创建数据目录失败");

    let secrets = default_store(data_dir.join("keystore.bin"));
    let mut config = EngineConfig::new(&data_dir, format!("127.0.0.1:{rc_port}"));
    config.rclone_path = Some(rclone);
    config.ready_timeout = Duration::from_secs(20);
    config.password_command = format!(
        "\"{}\" --key-file \"{}\"",
        pwcmd.display(),
        config.key_path().display()
    );

    let provider = RcloneProvider::new(config, secrets);
    provider.ensure_started().expect("引擎启动失败");
    let pid = provider.engine_pid().expect("引擎应已启动");

    let mut file = std::fs::File::create(&ready_file).expect("写 ready 文件失败");
    writeln!(file, "{pid}").expect("写 pid 失败");
    file.sync_all().expect("sync 失败");

    // 等待被强杀：不主动退出、不清理，确保测试观察到的是 Job Object 的效果
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}
