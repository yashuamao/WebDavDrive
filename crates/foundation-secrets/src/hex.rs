//! 极小的 hex 编解码：避免为存储令牌引入额外依赖。

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

pub fn decode(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16)?;
        let lo = (bytes[i + 1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
        i += 2;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_rejects_bad_input() {
        let data = b"\x00\x01\xfe\xff hello";
        let text = encode(data);
        assert_eq!(decode(&text).unwrap(), data);
        assert!(decode("abc").is_none(), "奇数长度应拒绝");
        assert!(decode("zz").is_none(), "非 hex 字符应拒绝");
        assert!(decode("").unwrap().is_empty());
    }
}
