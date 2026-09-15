//! 极简 rclone RC（Remote Control）HTTP 客户端。
//!
//! 只依赖标准库：回环 + JSON + Basic Auth，不需要通用 HTTP 客户端。
//! 端点语义与旧版一致：config/create|update|get|delete、mount/mount|unmount|listmounts、
//! operations/list、core/version（见 rclone docs/content/rc.md）。

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use foundation_core::{FoundationError, Result};
use serde_json::{json, Map, Value};

use super::MountRecord;

#[derive(Debug, Clone)]
pub struct RcloneRc {
    pub addr: String,
    pub user: String,
    pub password: String,
    pub timeout: Duration,
}

impl RcloneRc {
    pub fn new(
        addr: impl Into<String>,
        user: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            addr: addr.into(),
            user: user.into(),
            password: password.into(),
            timeout: Duration::from_secs(30),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn call(&self, endpoint: &str, params: Value) -> Result<Value> {
        let body = serde_json::to_vec(&params)?;
        let mut stream = TcpStream::connect(&self.addr).map_err(|err| {
            FoundationError::Process(format!(
                "{endpoint}: 无法连接 rclone RC {}：{err}",
                self.addr
            ))
        })?;
        let _ = stream.set_read_timeout(Some(self.timeout));
        let _ = stream.set_write_timeout(Some(self.timeout));

        let auth = base64_encode(format!("{}:{}", self.user, self.password).as_bytes());
        let head = format!(
            "POST /{} HTTP/1.1\r\nHost: {}\r\nAuthorization: Basic {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            endpoint.trim_start_matches('/'),
            self.addr,
            auth,
            body.len()
        );
        stream
            .write_all(head.as_bytes())
            .and_then(|_| stream.write_all(&body))
            .and_then(|_| stream.flush())
            .map_err(|err| FoundationError::Process(format!("{endpoint}: 发送请求失败：{err}")))?;

        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .map_err(|err| FoundationError::Process(format!("{endpoint}: 读取响应失败：{err}")))?;

        let (status, body_text) = parse_http_response(&raw)
            .map_err(|err| FoundationError::Process(format!("{endpoint}: {err}")))?;
        let payload: Value =
            serde_json::from_str(&body_text).unwrap_or_else(|_| json!({ "raw": body_text }));

        if !(200..300).contains(&status) {
            let detail = payload
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or(&body_text);
            return Err(FoundationError::Process(format!(
                "{endpoint}: HTTP {status}: {detail}"
            )));
        }
        if let Some(error) = payload.get("error").and_then(Value::as_str) {
            return Err(FoundationError::Process(format!("{endpoint}: {error}")));
        }
        Ok(payload)
    }

    // -- 便捷封装 -------------------------------------------------------------------

    pub fn version(&self) -> Result<Value> {
        self.call("core/version", json!({}))
    }

    /// 请求引擎自行退出；调用方仍应等待并在超时后强制回收进程。
    pub fn quit(&self) -> Result<Value> {
        self.call("core/quit", json!({}))
    }

    pub fn config_get(&self, name: &str) -> Result<Value> {
        self.call("config/get", json!({ "name": name }))
    }

    pub fn config_create(
        &self,
        name: &str,
        backend: &str,
        parameters: Map<String, Value>,
    ) -> Result<Value> {
        self.call(
            "config/create",
            json!({
                "name": name,
                "type": backend,
                "parameters": parameters,
                "opt": { "nonInteractive": true, "noOutput": true, "obscure": true }
            }),
        )
    }

    pub fn config_update(&self, name: &str, parameters: Map<String, Value>) -> Result<Value> {
        self.call(
            "config/update",
            json!({
                "name": name,
                "parameters": parameters,
                "opt": { "nonInteractive": true, "noOutput": true, "obscure": true }
            }),
        )
    }

    pub fn config_delete(&self, name: &str) -> Result<Value> {
        self.call("config/delete", json!({ "name": name }))
    }

    pub fn mount(
        &self,
        fs: &str,
        mount_point: &str,
        vfs: &Map<String, Value>,
        mount_opts: &Map<String, Value>,
        flat: &Map<String, Value>,
    ) -> Result<Value> {
        let mut params = Map::new();
        params.insert("fs".into(), Value::String(fs.to_string()));
        params.insert("mountPoint".into(), Value::String(mount_point.to_string()));
        if !vfs.is_empty() {
            params.insert("vfsOpt".into(), Value::Object(vfs.clone()));
        }
        if !mount_opts.is_empty() {
            params.insert("mountOpt".into(), Value::Object(mount_opts.clone()));
        }
        for (key, value) in flat {
            params.insert(key.clone(), value.clone());
        }
        self.call("mount/mount", Value::Object(params))
    }

    pub fn unmount(&self, mount_point: &str) -> Result<Value> {
        self.call("mount/unmount", json!({ "mountPoint": mount_point }))
    }

    pub fn list_mounts(&self) -> Result<Vec<MountRecord>> {
        let payload = self.call("mount/listmounts", json!({}))?;
        let entries = payload
            .get("mountPoints")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(entries
            .into_iter()
            .map(|entry| MountRecord {
                fs: entry
                    .get("Fs")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                mount_point: entry
                    .get("MountPoint")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            })
            .collect())
    }

    pub fn list_root(&self, fs: &str) -> Result<Value> {
        self.call("operations/list", json!({ "fs": fs, "remote": "" }))
    }
}

/// 解析 HTTP 响应：状态码 + 正文字符串（支持 chunked）。
fn parse_http_response(raw: &[u8]) -> std::result::Result<(u16, String), String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "响应缺少头部结束标记".to_string())?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| format!("无法解析状态行：{}", head.lines().next().unwrap_or("")))?;

    let body = &raw[split + 4..];
    let chunked = head.lines().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.starts_with("transfer-encoding:") && lower.contains("chunked")
    });
    let bytes = if chunked {
        decode_chunked(body)?
    } else {
        body.to_vec()
    };
    Ok((status, String::from_utf8_lossy(&bytes).to_string()))
}

fn decode_chunked(mut body: &[u8]) -> std::result::Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = body
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| "chunked 响应缺少长度行".to_string())?;
        let size_line = String::from_utf8_lossy(&body[..line_end]);
        let size_text = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| format!("chunked 长度非法：{size_text:?}"))?;
        body = &body[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if body.len() < size + 2 {
            return Err("chunked 数据不完整".into());
        }
        out.extend_from_slice(&body[..size]);
        body = &body[size + 2..];
    }
}

/// 标准 base64（Basic Auth 用；避免为 20 行代码引入依赖）。
fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(triple >> 18) as usize & 0x3f] as char);
        out.push(TABLE[(triple >> 12) as usize & 0x3f] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(triple >> 6) as usize & 0x3f] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[triple as usize & 0x3f] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn fake_server(response: String) -> (String, thread::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 1024];
            loop {
                let read = stream.read(&mut tmp).unwrap();
                if read == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..read]);
                if let Some(head_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..head_end]).to_lowercase();
                    let length: usize = head
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .and_then(|value| value.trim().parse().ok())
                        .unwrap_or(0);
                    if buf.len() >= head_end + 4 + length {
                        break;
                    }
                }
            }
            let _ = stream.write_all(response.as_bytes());
            buf
        });
        (addr, handle)
    }

    #[test]
    fn base64_matches_known_vector() {
        assert_eq!(base64_encode(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(base64_encode(b"a"), "YQ==");
        assert_eq!(base64_encode(b"ab"), "YWI=");
    }

    #[test]
    fn parses_plain_and_chunked_responses() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc";
        assert_eq!(parse_http_response(raw).unwrap(), (200, "abc".into()));

        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\n";
        assert_eq!(parse_http_response(chunked).unwrap(), (200, "abc".into()));
    }

    #[test]
    fn call_sends_basic_auth_and_parses_json() {
        let body = r#"{"version":"v1.75.1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let (addr, handle) = fake_server(response);
        let rc = RcloneRc::new(addr, "user", "pass");
        let value = rc.version().unwrap();
        assert_eq!(value["version"], "v1.75.1");

        let request = String::from_utf8_lossy(&handle.join().unwrap()).to_string();
        assert!(request.starts_with("POST /core/version HTTP/1.1"));
        assert!(request.contains("Authorization: Basic dXNlcjpwYXNz"));
        assert!(request.contains(r#""nonInteractive""#) == false); // version 请求没有多余参数
    }

    #[test]
    fn http_error_is_mapped_to_process_error() {
        let body = r#"{"error":"boom"}"#;
        let response = format!(
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let (addr, handle) = fake_server(response);
        let rc = RcloneRc::new(addr, "user", "pass");
        let err = rc.config_get("nas").unwrap_err();
        assert_eq!(err.code(), "process");
        assert!(err.to_string().contains("boom"), "{err}");
        handle.join().unwrap();
    }
}
