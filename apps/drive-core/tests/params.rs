//! AC-01..AC-03 / AC-11..AC-18：输入校验与挂载参数映射。

use drive_core::model::{Connection, ConnectionInput};
use drive_core::params::{build_mount_params, parse_extra_opts};
use foundation_secrets::{FileSecretStore, SecretStore};
use serde_json::json;
use std::sync::Arc;

fn base_connection() -> Connection {
    Connection {
        id: "webdav-1a2b3c4d".into(),
        name: "我的 NAS".into(),
        remote: "nas_1a2b3c4d".into(),
        url: "https://nas.example.com/dav".into(),
        vendor: "other".into(),
        user: "alice".into(),
        password_enc: None,
        drive: "X:".into(),
        volname: "我的 NAS".into(),
        network_mode: false,
        vfs_cache_mode: "writes".into(),
        dir_cache_time: "5m".into(),
        read_only: false,
        autostart: false,
        extra_opts: String::new(),
    }
}

fn memory_secret() -> Arc<dyn SecretStore> {
    let dir = std::env::temp_dir().join(format!("drive-params-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    Arc::new(FileSecretStore::new(dir.join("keystore.bin")))
}

#[test]
fn mount_params_map_ui_fields() {
    let mut conn = base_connection();
    conn.drive = "x:".into();
    conn.volname = "我的 NAS".into();
    conn.vfs_cache_mode = "full".into();
    conn.dir_cache_time = "10m".into();
    conn.read_only = true;
    conn.extra_opts = "--vfs-cache-max-size 10G".into();

    let params = build_mount_params(&conn).unwrap();
    assert_eq!(params.fs, "nas_1a2b3c4d:");
    assert_eq!(params.mount_point, "X:", "盘符必须大写");
    assert_eq!(params.vfs["CacheMode"], json!("full"));
    assert_eq!(params.vfs["DirCacheTime"], json!("10m"));
    assert_eq!(params.mount_opts["VolumeName"], json!("我的 NAS"));
    assert_eq!(params.flat["read_only"], json!(true));
    assert_eq!(params.flat["vfs_cache_max_size"], json!("10G"));
    assert!(!params.flat.contains_key("network_mode"));
}

#[test]
fn mount_points_accept_drive_star_and_directory() {
    let mut conn = base_connection();
    conn.drive = "*".into();
    assert_eq!(build_mount_params(&conn).unwrap().mount_point, "*");

    conn.drive = r"C:\mnt\nas".into();
    assert_eq!(
        build_mount_params(&conn).unwrap().mount_point,
        r"C:\mnt\nas"
    );

    for bad in [
        "XY",
        "",
        "1:",
        r"C:\mnt\..\windows",
        r"C:\mnt\nas|bad",
        r"mnt\nas",
    ] {
        conn.drive = bad.into();
        assert!(
            build_mount_params(&conn).is_err(),
            "非法挂载点必须被拒：{bad:?}"
        );
    }
}

#[test]
fn network_mode_is_mutually_exclusive() {
    let mut conn = base_connection();
    conn.network_mode = true;

    conn.drive = "*".into();
    assert!(build_mount_params(&conn).is_err(), "网络模式不允许自动分配");

    conn.drive = r"C:\mnt\nas".into();
    assert!(build_mount_params(&conn).is_err(), "网络模式不支持目录");

    conn.drive = "Y:".into();
    let params = build_mount_params(&conn).unwrap();
    assert_eq!(params.flat["network_mode"], json!(true));
}

#[test]
fn extra_opts_parse_like_cli_flags() {
    let opts = parse_extra_opts("--vfs-cache-max-size 10G --no-modtime --key=value").unwrap();
    assert_eq!(opts["vfs_cache_max_size"], json!("10G"));
    assert_eq!(opts["no_modtime"], json!(true), "裸 flag 视为 true");
    assert_eq!(opts["key"], json!("value"));

    let bools = parse_extra_opts("--no-modtime=true --checkers 4").unwrap();
    assert_eq!(bools["no_modtime"], json!(true));
    assert_eq!(bools["checkers"], json!(4));

    let quoted = parse_extra_opts(r#"--header "X-Custom: a b""#).unwrap();
    assert_eq!(quoted["header"], json!("X-Custom: a b"));

    assert!(parse_extra_opts("").unwrap().is_empty());
    assert!(parse_extra_opts(r#"--header "unterminated"#).is_err());
}

#[test]
fn blocked_and_invalid_inputs_are_rejected() {
    for blocked in [
        "--password-command calc.exe",
        r"--config C:\evil.conf",
        "--rc-no-auth",
        r"--log-file C:\evil.log",
    ] {
        assert!(
            parse_extra_opts(blocked).is_err(),
            "进程级 flag 必须被拒：{blocked}"
        );
    }

    let secrets = memory_secret();
    let mut input = ConnectionInput {
        name: Some("x".into()),
        url: Some("ftp://nas/dav".into()),
        ..Default::default()
    };
    assert!(Connection::from_input(input.clone(), None, secrets.as_ref()).is_err());

    input.url = Some("http://nas/dav".into());
    input.id = Some("bad\"id".into());
    assert!(Connection::from_input(input.clone(), None, secrets.as_ref()).is_err());

    input.id = None;
    let created = Connection::from_input(input, None, secrets.as_ref()).unwrap();
    assert!(created.id.starts_with("webdav-"));
    assert_eq!(created.id.len(), 15);
    assert_eq!(created.vfs_cache_mode, "writes");
    assert_eq!(created.drive, "X:");
    assert_eq!(
        created.remote,
        format!("x_{}", &created.id["webdav-".len()..])
    );
}

#[test]
fn password_lifecycle_keeps_or_clears() {
    let secrets = memory_secret();
    let input = ConnectionInput {
        name: Some("NAS".into()),
        url: Some("http://nas/dav".into()),
        password: Some("s3cret-汉字".into()),
        ..Default::default()
    };
    let created = Connection::from_input(input, None, secrets.as_ref()).unwrap();
    let token = created.password_enc.clone().expect("应保存密码令牌");
    assert!(secrets.unprotect(&token).unwrap() == "s3cret-汉字");

    // 留空 = 保持原密码
    let keep = ConnectionInput {
        id: Some(created.id.clone()),
        url: Some("http://nas/dav".into()),
        ..Default::default()
    };
    let updated = Connection::from_input(keep, Some(&created), secrets.as_ref()).unwrap();
    assert_eq!(updated.password_enc, created.password_enc);

    // 显式清除
    let clear = ConnectionInput {
        id: Some(created.id.clone()),
        url: Some("http://nas/dav".into()),
        clear_password: true,
        ..Default::default()
    };
    let cleared = Connection::from_input(clear, Some(&created), secrets.as_ref()).unwrap();
    assert!(cleared.password_enc.is_none());

    let conflict = ConnectionInput {
        id: Some(created.id.clone()),
        url: Some("http://nas/dav".into()),
        password: Some("replacement".into()),
        clear_password: true,
        ..Default::default()
    };
    assert!(Connection::from_input(conflict, Some(&created), secrets.as_ref()).is_err());
}
