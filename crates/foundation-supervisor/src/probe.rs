//! 就绪探测：TCP 或 HTTP。
//!
//! 外部服务启动后端口可用不代表 HTTP API 已就绪，所以需要显式探测；
//! HTTP 探测把 4xx 也视为"已就绪"——带认证的服务返回 401 恰恰说明它在工作。

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

/// 探测规格。
#[derive(Debug, Clone)]
pub enum Readiness {
    /// 不做探测（进程活着即视为就绪）。
    None,
    /// 能建立 TCP 连接即就绪。
    Tcp { addr: String },
    /// HTTP 有响应且状态码 < 500 即就绪。
    Http { url: String },
}

impl Default for Readiness {
    fn default() -> Self {
        Self::None
    }
}

/// 探测一次；Err 里是给人看的失败原因。
pub fn probe(readiness: &Readiness, timeout: Duration) -> Result<(), String> {
    match readiness {
        Readiness::None => Ok(()),
        Readiness::Tcp { addr } => {
            let socket = parse_addr(addr)?;
            TcpStream::connect_timeout(&socket, timeout)
                .map(|_| ())
                .map_err(|err| format!("TCP {addr} 连接失败：{err}"))
        }
        Readiness::Http { url } => http_ok(url, timeout),
    }
}

/// 端口是否已有进程监听（用于"立即退出"时区分端口冲突）。
pub fn port_open(addr: &str, timeout: Duration) -> bool {
    parse_addr(addr)
        .ok()
        .and_then(|socket| TcpStream::connect_timeout(&socket, timeout).ok())
        .is_some()
}

fn parse_addr(addr: &str) -> Result<SocketAddr, String> {
    addr.to_socket_addrs()
        .map_err(|err| format!("地址 {addr} 无法解析：{err}"))?
        .next()
        .ok_or_else(|| format!("地址 {addr} 没有解析结果"))
}

/// 拆出 (host:port, path)；只支持 http://。
pub fn split_http_url(url: &str) -> Result<(String, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("只支持 http:// 探测地址：{url}"))?;
    let (host, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    if host.is_empty() {
        return Err(format!("探测地址缺少主机：{url}"));
    }
    let host = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:80")
    };
    Ok((host, path.to_string()))
}

fn http_ok(url: &str, timeout: Duration) -> Result<(), String> {
    let (host, path) = split_http_url(url)?;
    let socket = parse_addr(&host)?;
    let mut stream = TcpStream::connect_timeout(&socket, timeout)
        .map_err(|err| format!("HTTP {url} 连接失败：{err}"))?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let request = format!("GET {path} HTTP/1.0\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|err| format!("HTTP {url} 写请求失败：{err}"))?;

    let mut buf = [0u8; 64];
    let read = stream
        .read(&mut buf)
        .map_err(|err| format!("HTTP {url} 无响应：{err}"))?;
    let head = String::from_utf8_lossy(&buf[..read]);
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| format!("HTTP {url} 响应无法解析：{}", head.trim()))?;
    if status < 500 {
        Ok(())
    } else {
        Err(format!("HTTP {url} 返回 {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn url_split_defaults_port_and_path() {
        assert_eq!(
            split_http_url("http://127.0.0.1:5572/core/version").unwrap(),
            ("127.0.0.1:5572".to_string(), "/core/version".to_string())
        );
        assert_eq!(
            split_http_url("http://example.com").unwrap(),
            ("example.com:80".to_string(), "/".to_string())
        );
        assert!(split_http_url("https://example.com").is_err());
    }

    #[test]
    fn tcp_probe_detects_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        assert!(probe(&Readiness::Tcp { addr }, Duration::from_millis(500)).is_ok());
    }

    #[test]
    fn http_probe_accepts_4xx_as_alive() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 256];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(b"HTTP/1.0 401 Unauthorized\r\nContent-Length: 0\r\n\r\n");
            }
        });
        let url = format!("http://{addr}/core/version");
        assert!(probe(&Readiness::Http { url }, Duration::from_millis(800)).is_ok());
        handle.join().unwrap();
    }

    #[test]
    fn http_probe_rejects_5xx() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 256];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(b"HTTP/1.0 503 Unavailable\r\nContent-Length: 0\r\n\r\n");
            }
        });
        let url = format!("http://{addr}/");
        assert!(probe(&Readiness::Http { url }, Duration::from_millis(800)).is_err());
        handle.join().unwrap();
    }
}
