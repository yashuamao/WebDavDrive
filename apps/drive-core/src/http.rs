//! 引擎更新用的极简 HTTPS 客户端（WinHTTP）。
//!
//! 为什么不用 reqwest/ureq：本仓库构建固定 cargo --offline，Cargo.lock 里没有 TLS 依赖树；
//! WinHTTP 是系统自带能力（TLS、证书链、系统代理都现成），正好覆盖「一次 GET」的需求。
//! 只实现需要的两种 GET：文本（release JSON / SHA256SUMS）与流式落盘（引擎 zip）。
//!
//! 复用 provider/rc.rs 的思路：不引入通用 HTTP 客户端，自己把协议边界收窄到需求本身。

use std::path::Path;
use std::time::Duration;

use foundation_core::{FoundationError, Result};

use crate::engine_update::{HttpText, UpdateTransport};

/// 只支持 http/https 绝对地址；返回 (是否 TLS, 主机, 端口, path?query)。
///
/// 单独抽出来是为了能单测：WinHTTP 自己解析 URL 需要 URL_COMPONENTS，对我们的地址格式没必要。
pub fn split_url(url: &str) -> std::result::Result<(bool, String, u16, String), String> {
    let (tls, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (false, rest)
    } else {
        return Err(format!("只支持 http/https 绝对地址：{url}"));
    };
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return Err(format!("地址缺少主机名：{url}"));
    }
    let default_port = if tls { 443 } else { 80 };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            let port = port
                .parse::<u16>()
                .map_err(|_| format!("端口非法：{port}"))?;
            (host.to_string(), port)
        }
        _ => (authority.to_string(), default_port),
    };
    if host.is_empty() {
        return Err(format!("地址缺少主机名：{url}"));
    }
    Ok((tls, host, port, path.to_string()))
}

#[cfg(not(windows))]
pub struct UnsupportedTransport;

#[cfg(not(windows))]
impl UpdateTransport for UnsupportedTransport {
    fn get_text(&self, _url: &str, _etag: Option<&str>) -> Result<HttpText> {
        Err(FoundationError::Platform(
            "引擎更新依赖 Windows 的 WinHTTP；当前平台不支持".into(),
        ))
    }

    fn download(&self, _url: &str, _dest: &Path) -> Result<()> {
        Err(FoundationError::Platform(
            "引擎更新依赖 Windows 的 WinHTTP；当前平台不支持".into(),
        ))
    }
}

#[cfg(windows)]
pub struct WinHttpTransport {
    user_agent: String,
    timeout: Duration,
    /// 文本响应上限：release JSON / SHA256SUMS 都很小，异常响应不能把内存吃光。
    max_text_bytes: usize,
}

#[cfg(windows)]
impl Default for WinHttpTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
impl WinHttpTransport {
    pub fn new() -> Self {
        Self {
            user_agent: format!("WebDavDrive/{}", env!("CARGO_PKG_VERSION")),
            timeout: Duration::from_secs(30),
            max_text_bytes: 4 * 1024 * 1024,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[cfg(windows)]
impl UpdateTransport for WinHttpTransport {
    fn get_text(&self, url: &str, etag: Option<&str>) -> Result<HttpText> {
        let mut body: Vec<u8> = Vec::new();
        let (status, response_etag) = self.request(url, etag, Sink::Text(&mut body))?;
        Ok(HttpText {
            status,
            body: String::from_utf8_lossy(&body).to_string(),
            etag: response_etag,
        })
    }

    fn download(&self, url: &str, dest: &Path) -> Result<()> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::File::create(dest)?;
        let outcome = self.request(url, None, Sink::File(&mut file));
        let status = match outcome {
            Ok((status, _)) if (200..300).contains(&status) => status,
            Ok((status, _)) => status,
            Err(err) => {
                // 不留半个包：调用方失败也不会误用残缺 zip。
                let _ = std::fs::remove_file(dest);
                return Err(err);
            }
        };
        if !(200..300).contains(&status) {
            let _ = std::fs::remove_file(dest);
            return Err(FoundationError::Process(format!(
                "下载失败：HTTP {status}（{url}）"
            )));
        }
        use std::io::Write;
        file.flush()?;
        Ok(())
    }
}

#[cfg(windows)]
enum Sink<'a> {
    Text(&'a mut Vec<u8>),
    File(&'a mut std::fs::File),
}

#[cfg(windows)]
impl WinHttpTransport {
    fn request(&self, url: &str, etag: Option<&str>, mut sink: Sink<'_>) -> Result<(u16, Option<String>)> {
        use std::ptr;
        use windows_sys::Win32::Networking::WinHttp::*;

        let (tls, host, port, path) = split_url(url).map_err(FoundationError::InvalidInput)?;
        let agent = wide(&self.user_agent);
        let host_wide = wide(&host);
        let path_wide = wide(&path);
        let verb = wide("GET");
        let millis = self.timeout.as_millis().min(i32::MAX as u128) as i32;

        unsafe {
            let mut session = WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                ptr::null(),
                ptr::null(),
                0,
            );
            if session.is_null() {
                // Windows 8.1 以前没有 AUTOMATIC_PROXY：退回系统默认代理配置。
                session = WinHttpOpen(
                    agent.as_ptr(),
                    WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                    ptr::null(),
                    ptr::null(),
                    0,
                );
            }
            if session.is_null() {
                return Err(last_error("WinHttpOpen"));
            }
            let session = Handle::new(session);
            WinHttpSetTimeouts(session.raw(), millis, millis, millis, millis);

            let connect = WinHttpConnect(session.raw(), host_wide.as_ptr(), port, 0);
            if connect.is_null() {
                return Err(last_error(format!("无法连接 {host}:{port}")));
            }
            let connect = Handle::new(connect);

            let flags = if tls { WINHTTP_FLAG_SECURE } else { 0 };
            let request = WinHttpOpenRequest(
                connect.raw(),
                verb.as_ptr(),
                path_wide.as_ptr(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                flags,
            );
            if request.is_null() {
                return Err(last_error("WinHttpOpenRequest"));
            }
            let request = Handle::new(request);

            // User-Agent 由 WinHttpOpen 提供；GitHub API 还要求 Accept，ETag 节流用 If-None-Match。
            let mut headers = String::from("Accept: application/vnd.github+json\r\n");
            if let Some(etag) = etag {
                headers.push_str("If-None-Match: ");
                headers.push_str(etag);
                headers.push_str("\r\n");
            }
            let headers_wide = wide(&headers);
            WinHttpAddRequestHeaders(
                request.raw(),
                headers_wide.as_ptr(),
                u32::MAX,
                WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE,
            );

            if WinHttpSendRequest(request.raw(), ptr::null(), 0, ptr::null(), 0, 0, 0) == 0 {
                return Err(last_error(format!("发送请求失败（{url}）")));
            }
            if WinHttpReceiveResponse(request.raw(), ptr::null_mut()) == 0 {
                return Err(last_error(format!("读取响应失败（{url}）")));
            }

            let mut status: u32 = 0;
            let mut status_size = std::mem::size_of::<u32>() as u32;
            WinHttpQueryHeaders(
                request.raw(),
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                ptr::null(),
                &mut status as *mut u32 as *mut std::ffi::c_void,
                &mut status_size,
                ptr::null_mut(),
            );
            let response_etag = query_header_text(request.raw(), WINHTTP_QUERY_ETAG);

            if (200..300).contains(&status) {
                read_body(request.raw(), &mut sink, self.max_text_bytes)?;
            }
            Ok((status as u16, response_etag))
        }
    }
}

#[cfg(windows)]
struct Handle(*mut std::ffi::c_void);

#[cfg(windows)]
impl Handle {
    /// 调用方已确认非空。
    unsafe fn new(raw: *mut std::ffi::c_void) -> Self {
        Self(raw)
    }

    fn raw(&self) -> *mut std::ffi::c_void {
        self.0
    }
}

#[cfg(windows)]
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Networking::WinHttp::WinHttpCloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn last_error(context: impl std::fmt::Display) -> FoundationError {
    FoundationError::Process(format!(
        "{context}：{}（检查网络/代理设置后重试）",
        std::io::Error::last_os_error()
    ))
}

#[cfg(windows)]
unsafe fn query_header_text(handle: *mut std::ffi::c_void, level: u32) -> Option<String> {
    use std::ffi::c_void;
    use std::ptr;
    use windows_sys::Win32::Networking::WinHttp::*;

    let mut size: u32 = 0;
    WinHttpQueryHeaders(
        handle,
        level,
        ptr::null(),
        ptr::null_mut(),
        &mut size,
        ptr::null_mut(),
    );
    if size == 0 {
        return None;
    }
    let mut buffer = vec![0u16; (size as usize / 2) + 1];
    if WinHttpQueryHeaders(
        handle,
        level,
        ptr::null(),
        buffer.as_mut_ptr() as *mut c_void,
        &mut size,
        ptr::null_mut(),
    ) == 0
    {
        return None;
    }
    let length = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..length]))
}

#[cfg(windows)]
unsafe fn read_body(
    handle: *mut std::ffi::c_void,
    sink: &mut Sink<'_>,
    limit: usize,
) -> Result<()> {
    use std::ffi::c_void;
    use std::io::Write;
    use windows_sys::Win32::Networking::WinHttp::*;

    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let mut available: u32 = 0;
        if WinHttpQueryDataAvailable(handle, &mut available) == 0 {
            return Err(last_error("读取响应长度失败"));
        }
        if available == 0 {
            return Ok(());
        }
        let want = available.min(buffer.len() as u32);
        let mut read: u32 = 0;
        if WinHttpReadData(handle, buffer.as_mut_ptr() as *mut c_void, want, &mut read) == 0 {
            return Err(last_error("读取响应内容失败"));
        }
        if read == 0 {
            return Ok(());
        }
        let chunk = &buffer[..read as usize];
        match sink {
            // 文本按字节累积、最后整体转 UTF-8：分块解码会把多字节字符切坏。
            Sink::Text(bytes) => {
                if bytes.len() + chunk.len() > limit {
                    return Err(FoundationError::Process(format!(
                        "响应体超过 {limit} 字节上限"
                    )));
                }
                bytes.extend_from_slice(chunk);
            }
            Sink::File(file) => file.write_all(chunk)?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_absolute_urls() {
        assert_eq!(
            split_url("https://api.github.com/repos/rclone/rclone/releases/latest").unwrap(),
            (
                true,
                "api.github.com".to_string(),
                443,
                "/repos/rclone/rclone/releases/latest".to_string()
            )
        );
        assert_eq!(
            split_url("http://127.0.0.1:8080/x").unwrap(),
            (false, "127.0.0.1".to_string(), 8080, "/x".to_string())
        );
        // 无路径时补根路径。
        assert_eq!(
            split_url("https://example.com").unwrap(),
            (true, "example.com".to_string(), 443, "/".to_string())
        );
        assert!(split_url("ftp://example.com/x").is_err());
        assert!(split_url("example.com/x").is_err());
        assert!(split_url("https://").is_err());
    }
}
