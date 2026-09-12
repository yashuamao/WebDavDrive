//! AC-04..AC-10：配置存储、密码保护、损坏隔离、旧格式迁移。

use std::path::PathBuf;
use std::sync::Arc;

use drive_core::model::{Connection, ConnectionInput};
use drive_core::store::ProfileStore;
use foundation_secrets::{FileSecretStore, SecretStore};
use serde_json::json;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("drive-store-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn open_store(dir: &PathBuf) -> ProfileStore {
    let secrets: Arc<dyn SecretStore> = Arc::new(FileSecretStore::new(dir.join("keystore.bin")));
    ProfileStore::open(dir, secrets).unwrap()
}

fn input(name: &str, url: &str) -> ConnectionInput {
    ConnectionInput {
        name: Some(name.into()),
        url: Some(url.into()),
        ..Default::default()
    }
}

#[test]
fn create_reveal_and_update_keeps_password() {
    let dir = temp_dir("lifecycle");
    let store = open_store(&dir);

    let mut first = input("我的 NAS", "https://nas.example.com/dav");
    first.password = Some("s3cret-汉字".into());
    let created = store.upsert(first).unwrap();
    assert!(created.id.starts_with("webdav-"));
    assert!(store.reveal_password(&created).unwrap() == "s3cret-汉字");

    // 明文不得出现在 profiles.json。文件后端（开发用）令牌即明文，这里只检查
    // DPAPI 生产路径（见下方 windows 专属测试）。
    let raw = std::fs::read_to_string(store.path()).unwrap();
    assert!(raw.contains("profiles"));

    // 留空密码更新不丢密码（AC-05）
    let mut edit = input("我的 NAS 2", "https://nas.example.com/dav");
    edit.id = Some(created.id.clone());
    let updated = store.upsert(edit).unwrap();
    assert_eq!(updated.password_enc, created.password_enc);
    assert_eq!(store.reveal_password(&updated).unwrap(), "s3cret-汉字");

    // 显式清除
    let mut clear = input("我的 NAS 2", "https://nas.example.com/dav");
    clear.id = Some(created.id.clone());
    clear.clear_password = true;
    store.upsert(clear).unwrap();
    let cleared = store.get(&created.id).unwrap();
    assert!(cleared.password_enc.is_none());
    assert_eq!(store.reveal_password(&cleared).unwrap(), "");
}

#[test]
fn corrupt_password_token_reports_error_instead_of_empty() {
    let dir = temp_dir("corrupt-password");
    let store = open_store(&dir);
    let mut connection = Connection::default();
    connection.id = "webdav-00000001".into();
    connection.name = "坏密码".into();
    connection.remote = "nas_00000001".into();
    connection.url = "http://nas/dav".into();
    connection.password_enc = Some("definitely-not-a-token".into());

    let err = store.reveal_password(&connection).unwrap_err();
    assert_eq!(err.code(), "secret_decrypt");
}

#[test]
fn corrupt_profiles_file_is_quarantined_and_starts_empty() {
    let dir = temp_dir("corrupt-file");
    let path = dir.join("profiles.json");
    std::fs::write(&path, "{ not json").unwrap();

    let store = open_store(&dir);
    assert!(store.list().is_empty(), "损坏后以空配置启动");
    let quarantined: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains("corrupt-"))
        .collect();
    assert_eq!(quarantined.len(), 1, "损坏文件必须被隔离");
    assert!(!path.exists(), "原路径应被移走");
}

#[test]
fn second_save_creates_backup() {
    let dir = temp_dir("backup");
    let store = open_store(&dir);
    store.upsert(input("A", "http://a/dav")).unwrap();
    store.upsert(input("B", "http://b/dav")).unwrap();
    assert!(dir.join("profiles.json.bak").exists());
}

#[test]
fn legacy_python_format_is_migrated() {
    let dir = temp_dir("legacy");
    let legacy = json!({
        "version": 1,
        "profiles": [{
            "id": "webdav-abcdef12",
            "name": "旧 NAS",
            "remote": "old_abcdef12",
            "url": "https://old.example.com/dav",
            "vendor": "other",
            "user": "old",
            "password_enc": null,
            "drive": "Z:",
            "volname": "旧 NAS",
            "network_mode": false,
            "vfs_cache_mode": "writes",
            "dir_cache_time": "5m",
            "read_only": false,
            "autostart": true,
            "extra_opts": ""
        }]
    });
    std::fs::write(
        dir.join("profiles.json"),
        serde_json::to_string_pretty(&legacy).unwrap(),
    )
    .unwrap();

    let store = open_store(&dir);
    let loaded = store.list();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, "webdav-abcdef12");
    assert_eq!(loaded[0].drive, "Z:");

    // 迁移后应是新信封格式，且旧文件被备份
    let raw = std::fs::read_to_string(store.path()).unwrap();
    assert!(raw.contains("\"data\""), "应写成新信封：{raw}");
    assert!(dir.join("profiles.json.bak").exists(), "旧文件应留 .bak");
}

#[test]
fn delete_removes_profile() {
    let dir = temp_dir("delete");
    let store = open_store(&dir);
    let created = store.upsert(input("A", "http://a/dav")).unwrap();
    let removed = store.delete(&created.id).unwrap();
    assert!(removed.is_some());
    assert!(store.list().is_empty());
    assert!(store.delete(&created.id).unwrap().is_none());
}

/// AC-04 / AC-25（生产路径）：Windows 上用机器范围 DPAPI，令牌是密文。
#[cfg(windows)]
#[test]
fn dpapi_profile_never_stores_plaintext_password() {
    let dir = temp_dir("dpapi");
    let secrets: Arc<dyn SecretStore> = foundation_secrets::default_store(dir.join("keystore.bin"));
    assert_eq!(secrets.name(), "dpapi-machine");
    let store = ProfileStore::open(&dir, secrets).unwrap();

    let mut first = input("DPAPI NAS", "https://nas.example.com/dav");
    first.password = Some("p@ssw0rd-汉字".into());
    let created = store.upsert(first).unwrap();
    assert_eq!(store.reveal_password(&created).unwrap(), "p@ssw0rd-汉字");

    let raw = std::fs::read_to_string(store.path()).unwrap();
    assert!(
        !raw.contains("p@ssw0rd-汉字"),
        "DPAPI 路径不得出现明文：{raw}"
    );
    let token = created.password_enc.unwrap();
    assert!(
        token.chars().all(|c| c.is_ascii_hexdigit()),
        "令牌应为 hex 密文"
    );
}
