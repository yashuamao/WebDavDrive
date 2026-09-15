//! AppService 用例编排测试（AC-10 删除防孤儿、密码损坏阻断、自启挂载）。
//! 用假 provider，验证服务层行为而不是引擎实现。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use drive_core::model::{Connection, ConnectionInput};
use drive_core::provider::{EngineStatus, MountProvider, MountRecord, ProbeReport};
use drive_core::{AppService, ProfileStore};
use foundation_core::{FoundationError, Result};
use foundation_secrets::{FileSecretStore, SecretStore};
use serde_json::json;

#[derive(Default)]
struct FakeProvider {
    mounts: Mutex<Vec<MountRecord>>,
    remotes: Mutex<Vec<String>>,
    delete_error: Mutex<Option<String>>,
    mount_error: Mutex<Option<String>>,
    list_fails: AtomicBool,
}

impl FakeProvider {
    fn mounted(&self) -> Vec<MountRecord> {
        self.mounts.lock().unwrap().clone()
    }

    fn remote_calls(&self) -> Vec<String> {
        self.remotes.lock().unwrap().clone()
    }
}

impl MountProvider for FakeProvider {
    fn id(&self) -> &'static str {
        "fake"
    }

    fn engine_status(&self) -> EngineStatus {
        EngineStatus {
            installed: true,
            path: Some("fake-rclone.exe".into()),
            version: Some("v-test".into()),
            running: true,
            rc_addr: Some("127.0.0.1:0".into()),
        }
    }

    fn ensure_started(&self) -> Result<()> {
        Ok(())
    }

    fn ensure_remote(&self, connection: &Connection, _password: &str) -> Result<()> {
        self.remotes.lock().unwrap().push(connection.remote.clone());
        Ok(())
    }

    fn probe(&self, _connection: &Connection, _password: &str) -> Result<ProbeReport> {
        Ok(ProbeReport {
            entries: 2,
            sample: vec!["docs".into(), "照片".into()],
        })
    }

    fn mount(&self, connection: &Connection, _password: &str) -> Result<MountRecord> {
        if let Some(message) = self.mount_error.lock().unwrap().clone() {
            return Err(FoundationError::Process(message));
        }
        let record = MountRecord {
            fs: format!("{}:", connection.remote),
            mount_point: if connection.drive == "*" {
                "Z:".into()
            } else {
                connection.drive.clone()
            },
        };
        self.mounts.lock().unwrap().push(record.clone());
        Ok(record)
    }

    fn unmount(&self, mount_point: &str) -> Result<()> {
        self.mounts
            .lock()
            .unwrap()
            .retain(|m| m.mount_point.to_lowercase() != mount_point.to_lowercase());
        Ok(())
    }

    fn list(&self) -> Result<Vec<MountRecord>> {
        if self.list_fails.load(Ordering::SeqCst) {
            return Err(FoundationError::Process("cannot reach engine".into()));
        }
        Ok(self.mounted())
    }

    fn delete_remote(&self, _remote: &str) -> Result<()> {
        if let Some(message) = self.delete_error.lock().unwrap().clone() {
            return Err(FoundationError::Process(message));
        }
        Ok(())
    }

    fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("drive-service-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn service(dir: &PathBuf, provider: Arc<FakeProvider>) -> AppService {
    let secrets: Arc<dyn SecretStore> = Arc::new(FileSecretStore::new(dir.join("keystore.bin")));
    let store = ProfileStore::open(dir, secrets).unwrap();
    AppService::new(store, provider)
}

fn input(name: &str, url: &str) -> ConnectionInput {
    ConnectionInput {
        name: Some(name.into()),
        url: Some(url.into()),
        password: Some("pw".into()),
        ..Default::default()
    }
}

#[test]
fn happy_path_save_probe_mount_unmount_delete() {
    let dir = temp_dir("happy");
    let provider = Arc::new(FakeProvider::default());
    let app = service(&dir, provider.clone());

    let saved = app.upsert(input("NAS", "http://nas/dav")).unwrap();
    assert_eq!(provider.remote_calls().len(), 1, "保存应同步 remote");

    let probe = app.probe(&saved.id).unwrap();
    assert_eq!(probe.entries, 2);
    assert_eq!(probe.sample[1], "照片");

    let record = app.mount(&saved.id).unwrap();
    assert_eq!(record.mount_point, "X:");
    assert_eq!(provider.mounted().len(), 1);

    app.unmount(&saved.id).unwrap();
    assert!(provider.mounted().is_empty(), "卸载应移除挂载记录");

    app.delete(&saved.id).unwrap();
    assert!(app.list().is_empty());
}

#[test]
fn delete_keeps_profile_when_remote_delete_fails() {
    let dir = temp_dir("orphan");
    let provider = Arc::new(FakeProvider::default());
    let app = service(&dir, provider.clone());

    let saved = app.upsert(input("NAS", "http://nas/dav")).unwrap();
    app.mount(&saved.id).unwrap();
    *provider.delete_error.lock().unwrap() = Some("cannot reach rclone RC".into());

    let err = app.delete(&saved.id).unwrap_err();
    assert_eq!(err.code(), "process");
    assert!(
        app.list().iter().any(|c| c.id == saved.id),
        "remote 删不掉时必须保留连接（防孤儿凭据）"
    );
    assert!(
        provider.mounted().is_empty(),
        "删除流程应先卸载已挂载的连接"
    );
}

#[test]
fn automatic_mount_point_is_resolved_by_remote_for_unmount_and_delete() {
    let dir = temp_dir("automatic-mount-point");
    let provider = Arc::new(FakeProvider::default());
    let app = service(&dir, provider.clone());

    let mut automatic = input("自动盘符", "http://auto/dav");
    automatic.drive = Some("*".into());
    let saved = app.upsert(automatic).unwrap();
    let mounted = app.mount(&saved.id).unwrap();
    assert_eq!(mounted.mount_point, "Z:");

    app.unmount(&saved.id).unwrap();
    assert!(provider.mounted().is_empty());

    app.mount(&saved.id).unwrap();
    app.delete(&saved.id).unwrap();
    assert!(provider.mounted().is_empty());
    assert!(app.get_view(&saved.id).is_none());
}

#[test]
fn delete_skips_unmount_check_when_engine_unreachable_but_still_protects() {
    let dir = temp_dir("orphan-list");
    let provider = Arc::new(FakeProvider::default());
    let app = service(&dir, provider.clone());

    let saved = app.upsert(input("NAS", "http://nas/dav")).unwrap();
    provider.list_fails.store(true, Ordering::SeqCst);
    *provider.delete_error.lock().unwrap() = Some("cannot reach rclone RC".into());

    assert!(app.delete(&saved.id).is_err());
    assert!(app.list().iter().any(|c| c.id == saved.id));
}

#[test]
fn corrupt_password_blocks_actions_before_provider_call() {
    let dir = temp_dir("corrupt-password");
    let profiles = json!({
        "version": 1,
        "data": { "profiles": [{
            "id": "webdav-00000001",
            "name": "坏密码",
            "remote": "nas_00000001",
            "url": "http://nas/dav",
            "vendor": "other",
            "user": "u",
            "password_enc": "garbage-token",
            "drive": "X:",
            "volname": "坏密码",
            "network_mode": false,
            "vfs_cache_mode": "writes",
            "dir_cache_time": "5m",
            "read_only": false,
            "autostart": false,
            "extra_opts": ""
        }]}
    });
    std::fs::write(
        dir.join("profiles.json"),
        serde_json::to_string(&profiles).unwrap(),
    )
    .unwrap();

    let provider = Arc::new(FakeProvider::default());
    let app = service(&dir, provider.clone());

    let err = app.mount("webdav-00000001").unwrap_err();
    assert_eq!(err.code(), "secret_decrypt");
    assert!(provider.mounted().is_empty(), "解密失败不得触发挂载");
}

#[test]
fn mount_all_autostart_only_mounts_flagged_profiles() {
    let dir = temp_dir("autostart");
    let provider = Arc::new(FakeProvider::default());
    let app = service(&dir, provider.clone());

    let mut flagged = input("自启", "http://a/dav");
    flagged.autostart = Some(true);
    app.upsert(flagged).unwrap();
    app.upsert(input("手动", "http://b/dav")).unwrap();

    let failures = app.mount_all_autostart();
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(provider.mounted().len(), 1);
    assert_eq!(provider.mounted()[0].mount_point, "X:");
}

#[test]
fn mount_all_autostart_reports_failures_without_aborting() {
    let dir = temp_dir("autostart-fail");
    let provider = Arc::new(FakeProvider::default());
    let app = service(&dir, provider.clone());

    let mut flagged = input("自启", "http://a/dav");
    flagged.autostart = Some(true);
    app.upsert(flagged).unwrap();
    *provider.mount_error.lock().unwrap() = Some("mount failed: no winfsp".into());

    let failures = app.mount_all_autostart();
    assert_eq!(failures.len(), 1);
    assert!(failures[0].1.contains("no winfsp"));
}
