//! 连接 → rclone 挂载参数映射。
//!
//! 对应 `docs/requirements.md` 的 BR-1（挂载点）、BR-7（附加参数）与 AC-11..AC-18。
//! 纯函数、无 IO，便于测试。

use foundation_core::{FoundationError, Result};
use serde_json::{Map, Value};

use crate::model::Connection;

/// 进程级危险 flag 黑名单：UI/API 输入不得成为命令执行或进程配置入口。
pub const BLOCKED_EXTRA_OPTS: [&str; 8] = [
    "password_command",
    "config",
    "rc_user",
    "rc_pass",
    "rc_addr",
    "rc_no_auth",
    "rc_serve",
    "log_file",
];

#[derive(Debug, Clone, PartialEq)]
pub struct MountParams {
    pub fs: String,
    pub mount_point: String,
    pub vfs: Map<String, Value>,
    pub mount_opts: Map<String, Value>,
    pub flat: Map<String, Value>,
}

/// 校验并规范化挂载点：`X:` / `*` / 绝对目录（如 `C:\mnt\nas`）。
pub fn normalize_mount_point(value: &str, network_mode: bool) -> Result<String> {
    let raw = value.trim();
    if raw.is_empty() {
        return Err(FoundationError::InvalidInput(
            "挂载点必须是单个字母加冒号（如 X:）、*，或绝对目录路径（如 C:\\mnt\\nas）".into(),
        ));
    }

    if raw == "*" {
        if network_mode {
            return Err(FoundationError::InvalidInput(
                "网络驱动器模式必须指定具体盘符（rclone 在 --network-mode 下不支持自动分配）"
                    .into(),
            ));
        }
        return Ok("*".to_string());
    }

    if is_drive_letter(raw) {
        return Ok(raw.to_ascii_uppercase());
    }

    if network_mode {
        return Err(FoundationError::InvalidInput(
            "网络驱动器模式不支持目录挂载点（--network-mode 只接受盘符）".into(),
        ));
    }

    let bytes = raw.as_bytes();
    let looks_absolute = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\');
    if !looks_absolute {
        return Err(FoundationError::InvalidInput(format!(
            "挂载点必须是单个字母加冒号（如 X:）、*，或绝对目录路径（如 C:\\mnt\\nas），收到：{value:?}"
        )));
    }

    let tail = raw[2..].trim_end_matches(['\\', '/']);
    if tail.len() < 2 {
        return Err(FoundationError::InvalidInput(format!(
            "目录挂载点不能是盘符根目录：{value:?}"
        )));
    }
    if raw.split(['\\', '/']).any(|part| part == "..") {
        return Err(FoundationError::InvalidInput(format!(
            "目录挂载点包含相对路径片段：{value:?}"
        )));
    }
    if tail
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control())
    {
        return Err(FoundationError::InvalidInput(format!(
            "目录挂载点包含非法字符：{value:?}"
        )));
    }

    Ok(raw.to_string())
}

fn is_drive_letter(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// 界面字段 → RC 参数。
pub fn build_mount_params(conn: &Connection) -> Result<MountParams> {
    let mount_point = normalize_mount_point(&conn.drive, conn.network_mode)?;

    let mut vfs = Map::new();
    if !conn.vfs_cache_mode.is_empty() {
        vfs.insert(
            "CacheMode".into(),
            Value::String(conn.vfs_cache_mode.clone()),
        );
    }
    if !conn.dir_cache_time.is_empty() {
        vfs.insert(
            "DirCacheTime".into(),
            Value::String(conn.dir_cache_time.clone()),
        );
    }

    let mut mount_opts = Map::new();
    let volname = if conn.volname.trim().is_empty() {
        conn.name.trim()
    } else {
        conn.volname.trim()
    };
    if !volname.is_empty() {
        mount_opts.insert("VolumeName".into(), Value::String(volname.to_string()));
    }

    let mut flat = Map::new();
    if conn.network_mode {
        flat.insert("network_mode".into(), Value::Bool(true));
    }
    if conn.read_only {
        flat.insert("read_only".into(), Value::Bool(true));
    }
    for (key, value) in parse_extra_opts(&conn.extra_opts)? {
        flat.insert(key, value);
    }

    Ok(MountParams {
        fs: format!("{}:", conn.remote),
        mount_point,
        vfs,
        mount_opts,
        flat,
    })
}

/// `--vfs-cache-max-size 10G --no-modtime` → `{"vfs_cache_max_size":"10G","no_modtime":true}`。
pub fn parse_extra_opts(text: &str) -> Result<Map<String, Value>> {
    let mut out = Map::new();
    if text.trim().is_empty() {
        return Ok(out);
    }
    let tokens = split_tokens(text)?;

    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        let Some(body) = token.strip_prefix("--") else {
            index += 1;
            continue;
        };
        let key = body.split('=').next().unwrap_or(body).replace('-', "_");
        if BLOCKED_EXTRA_OPTS.contains(&key.as_str()) {
            return Err(FoundationError::InvalidInput(format!(
                "附加参数不允许使用 --{}：该选项属于进程级配置",
                body.split('=').next().unwrap_or(body)
            )));
        }

        if let Some((_, value)) = body.split_once('=') {
            out.insert(key, coerce(value));
        } else if index + 1 < tokens.len() && !tokens[index + 1].starts_with("--") {
            out.insert(key, coerce(&tokens[index + 1]));
            index += 1;
        } else {
            out.insert(key, Value::Bool(true));
        }
        index += 1;
    }
    Ok(out)
}

fn coerce(raw: &str) -> Value {
    if raw.eq_ignore_ascii_case("true") {
        Value::Bool(true)
    } else if raw.eq_ignore_ascii_case("false") {
        Value::Bool(false)
    } else if let Ok(number) = raw.parse::<i64>() {
        Value::from(number)
    } else {
        Value::String(raw.to_string())
    }
}

/// shell 风格分词：支持单/双引号与反斜杠转义（对齐旧版 shlex.split 的常用行为）。
fn split_tokens(text: &str) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = text.chars();

    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '\\' if !in_single => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }

    if in_single || in_double {
        return Err(FoundationError::InvalidInput("附加参数引号不匹配".into()));
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}
