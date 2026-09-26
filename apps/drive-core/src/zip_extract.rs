//! 极简 zip 读取：只从官方 rclone 包里取出 rclone.exe。
//!
//! 为什么不加 zip crate：仓库构建固定 cargo --offline，zip 及其依赖树不在 Cargo.lock 里；
//! flate2 已在依赖树里，手写中央目录解析只有一百来行，而且能单测。
//! 只支持 stored(0) 与 deflate(8) 两种压缩方式——rclone 官方包和 Windows 资源管理器
//! 生成的 zip 都是这两种。
//!
//! 不校验 zip 内 CRC：官方包的外层 SHA256 已经在 engine_update 里校验过，本地文件
//! 安装路径用的是 PE 头 + 后续 rclone 自检（core/version）。

use std::fs::{self, File};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;

use foundation_core::{FoundationError, Result};

const EOCD_SIGNATURE: u32 = 0x0605_4b50;
const EOCD64_LOCATOR_SIGNATURE: u32 = 0x0706_4b50;
const EOCD64_SIGNATURE: u32 = 0x0606_4b50;
const CENTRAL_SIGNATURE: u32 = 0x0201_4b50;
const LOCAL_SIGNATURE: u32 = 0x0403_4b50;
const ZIP64_EXTRA_ID: u16 = 0x0001;
/// EOCD 固定 22 字节，前面最多再跟 64KB 注释。
const EOCD_SEARCH_LIMIT: u64 = 22 + 65_535;
const U32_MAX: u64 = u32::MAX as u64;

fn damaged(message: impl Into<String>) -> FoundationError {
    FoundationError::DataCorrupted(format!("zip：{}", message.into()))
}

fn read_u16(reader: &mut impl Read) -> Result<u16> {
    let mut bytes = [0u8; 2];
    reader
        .read_exact(&mut bytes)
        .map_err(|err| damaged(format!("读取失败：{err}")))?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32(reader: &mut impl Read) -> Result<u32> {
    let mut bytes = [0u8; 4];
    reader
        .read_exact(&mut bytes)
        .map_err(|err| damaged(format!("读取失败：{err}")))?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(reader: &mut impl Read) -> Result<u64> {
    let mut bytes = [0u8; 8];
    reader
        .read_exact(&mut bytes)
        .map_err(|err| damaged(format!("读取失败：{err}")))?;
    Ok(u64::from_le_bytes(bytes))
}

/// 中央目录里的一个成员。
#[derive(Debug, Clone, PartialEq)]
struct Entry {
    name: String,
    method: u16,
    compressed_size: u64,
    local_offset: u64,
}

/// 定位中央目录：普通 EOCD，必要时退回 zip64 EOCD。
fn central_directory(file: &mut File) -> Result<(u64, u64)> {
    let length = file.metadata()?.len();
    let window = length.min(EOCD_SEARCH_LIMIT);
    if window < 22 {
        return Err(damaged("文件太小，不是有效 zip"));
    }
    let base = length - window;
    file.seek(SeekFrom::Start(base))?;
    let mut buffer = vec![0u8; window as usize];
    file.read_exact(&mut buffer)?;
    let position = buffer
        .windows(4)
        .rposition(|bytes| bytes == EOCD_SIGNATURE.to_le_bytes())
        .ok_or_else(|| damaged("找不到中央目录结尾记录（不是 zip 或已截断）"))?;

    let mut cursor = Cursor::new(&buffer[position..]);
    cursor.seek(SeekFrom::Start(10))?;
    let entries = read_u16(&mut cursor)? as u64;
    let directory_size = read_u32(&mut cursor)? as u64;
    let directory_offset = read_u32(&mut cursor)? as u64;
    if entries != u16::MAX as u64 && directory_offset != U32_MAX && directory_size != U32_MAX {
        return Ok((entries, directory_offset));
    }

    // zip64：定位器紧跟在 EOCD 前面（固定 20 字节）。
    let eocd_offset = base + position as u64;
    let locator_offset = eocd_offset
        .checked_sub(20)
        .ok_or_else(|| damaged("缺少 zip64 定位器"))?;
    file.seek(SeekFrom::Start(locator_offset))?;
    let mut locator = [0u8; 20];
    file.read_exact(&mut locator)?;
    let mut cursor = Cursor::new(&locator[..]);
    if read_u32(&mut cursor)? != EOCD64_LOCATOR_SIGNATURE {
        return Err(damaged("zip64 定位器签名不对"));
    }
    cursor.seek(SeekFrom::Start(8))?;
    let eocd64_offset = read_u64(&mut cursor)?;

    file.seek(SeekFrom::Start(eocd64_offset))?;
    let mut record = [0u8; 56];
    file.read_exact(&mut record)?;
    let mut cursor = Cursor::new(&record[..]);
    if read_u32(&mut cursor)? != EOCD64_SIGNATURE {
        return Err(damaged("zip64 结尾签名不对"));
    }
    cursor.seek(SeekFrom::Start(32))?;
    let entries = read_u64(&mut cursor)?;
    let _directory_size = read_u64(&mut cursor)?;
    let directory_offset = read_u64(&mut cursor)?;
    Ok((entries, directory_offset))
}

/// 从 zip64 扩展字段里补齐被写成 0xFFFFFFFF 的字段。
fn apply_zip64_extra(entry: &mut Entry, extra: &[u8], needs: (bool, bool, bool)) {
    let (need_compressed, need_size, need_offset) = needs;
    let mut cursor = Cursor::new(extra);
    while let Ok(id) = read_u16(&mut cursor) {
        let Ok(size) = read_u16(&mut cursor) else { return };
        let mut data = vec![0u8; size as usize];
        if cursor.read_exact(&mut data).is_err() {
            return;
        }
        if id == ZIP64_EXTRA_ID {
            let mut data_cursor = Cursor::new(&data[..]);
            if need_size {
                if let Ok(value) = read_u64(&mut data_cursor) {
                    // 未压缩长度不用，只消费掉以保持顺序
                    let _ = value;
                }
            }
            if need_compressed {
                if let Ok(value) = read_u64(&mut data_cursor) {
                    entry.compressed_size = value;
                }
            }
            if need_offset {
                if let Ok(value) = read_u64(&mut data_cursor) {
                    entry.local_offset = value;
                }
            }
            return;
        }
    }
}

/// 在中央目录里找第一个「文件名等于 rclone.exe」的成员（大小写不敏感）。
fn find_rclone_exe(file: &mut File, directory_offset: u64, entries: u64) -> Result<Option<Entry>> {
    file.seek(SeekFrom::Start(directory_offset))?;
    for _ in 0..entries {
        let mut header = [0u8; 46];
        file.read_exact(&mut header)?;
        let mut cursor = Cursor::new(&header[..]);
        if read_u32(&mut cursor)? != CENTRAL_SIGNATURE {
            return Err(damaged("中央目录项签名不对"));
        }
        cursor.seek(SeekFrom::Start(10))?;
        let method = read_u16(&mut cursor)?;
        cursor.seek(SeekFrom::Start(20))?;
        let compressed = read_u32(&mut cursor)? as u64;
        let uncompressed = read_u32(&mut cursor)? as u64;
        let name_length = read_u16(&mut cursor)? as usize;
        let extra_length = read_u16(&mut cursor)? as usize;
        let comment_length = read_u16(&mut cursor)? as usize;
        cursor.seek(SeekFrom::Start(42))?;
        let local_offset = read_u32(&mut cursor)? as u64;

        let mut name_bytes = vec![0u8; name_length];
        file.read_exact(&mut name_bytes)?;
        let mut extra = vec![0u8; extra_length];
        file.read_exact(&mut extra)?;
        if comment_length > 0 {
            let mut comment = vec![0u8; comment_length];
            file.read_exact(&mut comment)?;
        }

        let mut entry = Entry {
            name: String::from_utf8_lossy(&name_bytes).to_string(),
            method,
            compressed_size: compressed,
            local_offset,
        };
        if compressed == U32_MAX || uncompressed == U32_MAX || local_offset == U32_MAX {
            apply_zip64_extra(
                &mut entry,
                &extra,
                (compressed == U32_MAX, uncompressed == U32_MAX, local_offset == U32_MAX),
            );
        }

        let file_name = entry
            .name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(entry.name.as_str());
        if file_name.eq_ignore_ascii_case("rclone.exe") {
            return Ok(Some(entry));
        }
    }
    Ok(None)
}

/// 把某个成员解压到 dest（先删后写，避免留下半个文件）。
fn extract_entry(file: &mut File, entry: &Entry, dest: &Path) -> Result<()> {
    file.seek(SeekFrom::Start(entry.local_offset))?;
    let mut header = [0u8; 30];
    file.read_exact(&mut header)?;
    let mut cursor = Cursor::new(&header[..]);
    if read_u32(&mut cursor)? != LOCAL_SIGNATURE {
        return Err(damaged("本地文件头签名不对"));
    }
    cursor.seek(SeekFrom::Start(26))?;
    let name_length = read_u16(&mut cursor)? as usize;
    let extra_length = read_u16(&mut cursor)? as usize;
    file.seek(SeekFrom::Current((name_length + extra_length) as i64))?;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let _ = fs::remove_file(dest);
    let mut output = File::create(dest)?;
    // 长度取中央目录里的压缩长度：本地头在带 data descriptor 时会是 0。
    let mut payload = Read::by_ref(file).take(entry.compressed_size);
    match entry.method {
        0 => {
            std::io::copy(&mut payload, &mut output)?;
        }
        8 => {
            let mut decoder = flate2::read::DeflateDecoder::new(payload);
            std::io::copy(&mut decoder, &mut output)?;
        }
        other => {
            return Err(damaged(format!(
                "不支持的压缩方式 {other}（只支持 stored/deflate）"
            )))
        }
    }
    output.flush()?;
    Ok(())
}

/// 从 zip 里解出 rclone.exe 到 dest，返回解出的成员名。
pub fn extract_rclone_exe(archive: &Path, dest: &Path) -> Result<String> {
    let mut file = File::open(archive)
        .map_err(|err| damaged(format!("无法打开 {}：{err}", archive.display())))?;
    let (entries, directory_offset) = central_directory(&mut file)?;
    let entry = find_rclone_exe(&mut file, directory_offset, entries)?
        .ok_or_else(|| damaged(format!("{} 里没有 rclone.exe", archive.display())))?;
    extract_entry(&mut file, &entry, dest)?;

    // 解出来的必须是 PE：挡住把错误页面备注成 .zip 这类情况。
    let mut head = [0u8; 2];
    let mut extracted = File::open(dest)?;
    if extracted.read_exact(&mut head).is_err() || &head != b"MZ" {
        return Err(damaged(format!(
            "{} 里的 {} 不是 Windows 可执行文件",
            archive.display(),
            entry.name
        )));
    }
    Ok(entry.name)
}

/// 测试辅助：造一个最小 zip（CRC 写 0：读取端不校验 CRC，外层有 SHA256）。
/// 供本模块与 engine_update 的单测共用。
#[cfg(test)]
pub(crate) mod test_zip {
    use super::*;

    pub(crate) fn build_zip(members: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let mut central: Vec<u8> = Vec::new();
        for (name, data, deflate) in members {
            let offset = out.len() as u32;
            let payload: Vec<u8> = if *deflate {
                let mut encoder = flate2::write::DeflateEncoder::new(
                    Vec::new(),
                    flate2::Compression::default(),
                );
                encoder.write_all(data).unwrap();
                encoder.finish().unwrap()
            } else {
                data.to_vec()
            };
            let method: u16 = if *deflate { 8 } else { 0 };

            out.extend_from_slice(&LOCAL_SIGNATURE.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0u8; 4]);
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&payload);

            central.extend_from_slice(&CENTRAL_SIGNATURE.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&[0u8; 4]);
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let directory_offset = out.len() as u32;
        let directory_size = central.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&EOCD_SIGNATURE.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(members.len() as u16).to_le_bytes());
        out.extend_from_slice(&(members.len() as u16).to_le_bytes());
        out.extend_from_slice(&directory_size.to_le_bytes());
        out.extend_from_slice(&directory_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::test_zip::build_zip;
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("zip-read-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const ENGINE: &[u8] = b"MZ\x90\x00fake-rclone-binary";

    #[test]
    fn extracts_deflated_engine_from_nested_folder() {
        let dir = temp_dir("deflate");
        let archive = dir.join("rclone-v1.75.1-windows-amd64.zip");
        let bytes = build_zip(&[
            ("rclone-v1.75.1-windows-amd64/README.txt", b"docs", true),
            ("rclone-v1.75.1-windows-amd64/rclone.exe", ENGINE, true),
        ]);
        fs::write(&archive, &bytes).unwrap();

        let dest = dir.join("out").join("rclone.exe");
        let name = extract_rclone_exe(&archive, &dest).unwrap();
        assert_eq!(name, "rclone-v1.75.1-windows-amd64/rclone.exe");
        assert_eq!(fs::read(&dest).unwrap(), ENGINE);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn extracts_stored_engine() {
        let dir = temp_dir("stored");
        let archive = dir.join("engine.zip");
        fs::write(&archive, build_zip(&[("rclone.exe", ENGINE, false)])).unwrap();
        let dest = dir.join("rclone.exe");
        assert_eq!(extract_rclone_exe(&archive, &dest).unwrap(), "rclone.exe");
        assert_eq!(fs::read(&dest).unwrap(), ENGINE);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reports_missing_engine_and_non_zip() {
        let dir = temp_dir("missing");
        let archive = dir.join("other.zip");
        fs::write(&archive, build_zip(&[("notes.txt", b"hi", true)])).unwrap();
        let err = extract_rclone_exe(&archive, &dir.join("out.exe")).unwrap_err();
        assert!(err.to_string().contains("没有 rclone.exe"), "{err}");

        let not_a_zip = dir.join("broken.zip");
        fs::write(&not_a_zip, b"<html>error page</html>").unwrap();
        assert!(extract_rclone_exe(&not_a_zip, &dir.join("out.exe")).is_err());

        let renamed = dir.join("renamed.zip");
        fs::write(&renamed, build_zip(&[("rclone.exe", b"not a pe", false)])).unwrap();
        let err = extract_rclone_exe(&renamed, &dir.join("out.exe")).unwrap_err();
        assert!(err.to_string().contains("不是 Windows 可执行文件"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_eocd_after_large_payload() {
        // 中央目录前面放 100KB 数据，确认 EOCD 搜索窗口与偏移都算对了。
        let dir = temp_dir("large");
        let pad = vec![9u8; 100 * 1024];
        let archive = dir.join("large.zip");
        fs::write(
            &archive,
            build_zip(&[
                ("rclone-v1.75.1-windows-amd64/rclone.exe", ENGINE, true),
                ("rclone-v1.75.1-windows-amd64/other.bin", &pad, true),
            ]),
        )
        .unwrap();
        let dest = dir.join("rclone.exe");
        extract_rclone_exe(&archive, &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), ENGINE);
        let _ = fs::remove_dir_all(&dir);
    }
}
