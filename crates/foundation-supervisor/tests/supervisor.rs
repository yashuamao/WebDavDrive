#![cfg(windows)]

use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use foundation_core::ChildGuard;
use foundation_supervisor::{ManagedChild, Readiness, Spec};

/// 一个能活几秒、不需要控制台输入的进程。
fn ping_args() -> Vec<String> {
    vec![
        "/C".into(),
        "ping".into(),
        "-n".into(),
        "6".into(),
        "127.0.0.1".into(),
    ]
}

fn ping_spec() -> Spec {
    let mut spec = Spec::new("cmd");
    spec.args = ping_args();
    spec.ready_timeout = Duration::from_secs(5);
    spec.stop_grace = Duration::from_secs(3);
    spec
}

#[test]
fn spawn_wait_stop_plain_process() {
    let spec = ping_spec();
    let mut child = ManagedChild::spawn_ready(&spec).unwrap();
    assert!(child.pid() > 0);
    assert!(child.is_running());
    child.stop().unwrap();
    assert!(!child.is_running(), "stop 之后进程必须结束");
}

#[test]
fn tcp_readiness_waits_for_listener() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();

    let mut spec = ping_spec();
    spec.readiness = Readiness::Tcp { addr };
    let mut child = ManagedChild::spawn_ready(&spec).unwrap();
    assert!(child.is_running());
    child.stop().unwrap();
}

#[test]
fn early_exit_is_reported_as_process_error() {
    let mut spec = Spec::new("cmd");
    spec.args = vec!["/C".into(), "exit".into(), "3".into()];
    // 用一个永远不会就绪的探测，才能真正观察到"进程先退出"
    spec.readiness = Readiness::Tcp {
        addr: "127.0.0.1:1".into(),
    };
    spec.ready_timeout = Duration::from_secs(3);

    let err = ManagedChild::spawn_ready(&spec).unwrap_err();
    assert_eq!(err.code(), "process");
    assert!(
        err.to_string().contains("立即退出"),
        "错误应说明进程提前退出：{err}"
    );
}

#[test]
fn missing_program_fails_cleanly() {
    let spec = Spec::new(r"C:\definitely\not\here\nope.exe");
    let err = ManagedChild::spawn(&spec).unwrap_err();
    assert_eq!(err.code(), "process");
}

struct RejectingGuard;

impl ChildGuard for RejectingGuard {
    fn assign(&self, _pid: u32) -> bool {
        false
    }

    fn name(&self) -> &'static str {
        "rejecting-test-guard"
    }
}

#[test]
fn rejected_lifecycle_guard_fails_closed() {
    let mut spec = ping_spec();
    spec.guard = Some(Arc::new(RejectingGuard));

    let err = ManagedChild::spawn(&spec).unwrap_err();
    assert_eq!(err.code(), "platform");
    assert!(err.to_string().contains("已终止子进程"), "{err}");
}
