use foundation_config::{FileStore, LoadOutcome, SCHEMA_VERSION};
use serde_json::json;
use std::path::PathBuf;

fn temp_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("foundation-config-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn missing_and_empty_are_missing() {
    let dir = temp_dir("missing");
    let store = FileStore::new(dir.join("profiles.json"));
    assert_eq!(store.load().unwrap(), LoadOutcome::Missing);

    std::fs::write(store.path(), "").unwrap();
    assert_eq!(store.load().unwrap(), LoadOutcome::Missing);
}

#[test]
fn roundtrip_writes_versioned_envelope() {
    let dir = temp_dir("roundtrip");
    let store = FileStore::new(dir.join("profiles.json"));
    let data = json!({"profiles": [{"id": "webdav-1", "name": "NAS"}]});

    store.save(&data).unwrap();
    match store.load().unwrap() {
        LoadOutcome::Loaded(envelope) => {
            assert_eq!(envelope.version, SCHEMA_VERSION);
            assert_eq!(envelope.data, data);
        }
        other => panic!("期望 Loaded，得到 {other:?}"),
    }
    // 临时文件必须清理干净
    assert!(!store.tmp_path().exists());
}

#[test]
fn second_save_creates_backup_of_previous_content() {
    let dir = temp_dir("backup");
    let store = FileStore::new(dir.join("profiles.json"));
    store.save(&json!({"n": 1})).unwrap();
    assert!(!store.backup_path().exists(), "首次保存没有上一版可备份");

    store.save(&json!({"n": 2})).unwrap();
    assert!(store.backup_path().exists());
    let backup = std::fs::read_to_string(store.backup_path()).unwrap();
    assert!(backup.contains("\"n\": 1"), "备份应是上一版内容：{backup}");
}

#[test]
fn corrupt_json_is_quarantined_not_overwritten() {
    let dir = temp_dir("corrupt");
    let store = FileStore::new(dir.join("profiles.json"));
    std::fs::write(store.path(), "{ this is not json").unwrap();

    match store.load().unwrap() {
        LoadOutcome::Quarantined { backup, .. } => {
            assert!(backup.exists(), "损坏文件应被隔离");
            assert!(!store.path().exists(), "原路径应被移走");
            let moved = std::fs::read_to_string(&backup).unwrap();
            assert!(moved.contains("not json"));
        }
        other => panic!("期望 Quarantined，得到 {other:?}"),
    }
}

#[test]
fn non_object_top_level_is_quarantined() {
    let dir = temp_dir("non-object");
    let store = FileStore::new(dir.join("profiles.json"));
    std::fs::write(store.path(), "[1, 2, 3]").unwrap();
    match store.load().unwrap() {
        LoadOutcome::Quarantined { .. } => {}
        other => panic!("期望 Quarantined，得到 {other:?}"),
    }
}

#[test]
fn missing_envelope_fields_are_quarantined() {
    let dir = temp_dir("envelope");
    let store = FileStore::new(dir.join("profiles.json"));
    std::fs::write(store.path(), r#"{"value": 1}"#).unwrap();
    match store.load().unwrap() {
        LoadOutcome::Quarantined { .. } => {}
        other => panic!("缺 version/data 应视为损坏，得到 {other:?}"),
    }
}

#[test]
fn newer_version_loads_but_is_flagged_for_caller() {
    let dir = temp_dir("newer");
    let store = FileStore::new(dir.join("profiles.json"));
    std::fs::write(store.path(), r#"{"version": 99, "data": {"x": 1}}"#).unwrap();

    match store.load().unwrap() {
        LoadOutcome::Loaded(envelope) => {
            // 未来版本必须能读出来交给调用方拒绝写入，而不是隔离掉
            assert_eq!(envelope.version, 99);
            assert_eq!(envelope.data, json!({"x": 1}));
        }
        other => panic!("期望 Loaded（供调用方处理版本），得到 {other:?}"),
    }
}
