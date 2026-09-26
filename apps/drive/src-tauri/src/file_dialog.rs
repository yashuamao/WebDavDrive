//! 系统文件选择框：为「从本地文件安装引擎」挑选 rclone.exe 或官方 zip。
//!
//! 为什么不用 tauri-plugin-dialog：仓库构建固定 cargo --offline，插件不在 Cargo.lock 里；
//! comdlg32 的 GetOpenFileNameW 是系统自带能力，本项目本来就在用 windows-sys。

/// 打开文件选择框，返回用户选中的路径；取消或失败返回 None。
///
/// owner 传主窗口 HWND（0 = 无属主窗口），保证对话框出现在应用窗口前面。
#[cfg(windows)]
pub fn pick_engine_file(owner: isize) -> Result<Option<String>, String> {
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OPENFILENAMEW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR,
        OFN_PATHMUSTEXIST,
    };

    const BUFFER_LEN: usize = 32_768;
    let mut buffer = vec![0u16; BUFFER_LEN];
    // 过滤器必须是双 \0 结尾的成对字符串。
    let filter: Vec<u16> = "引擎文件 (*.exe;*.zip)\0*.exe;*.zip\0所有文件 (*.*)\0*.*\0\0"
        .encode_utf16()
        .collect();
    let title: Vec<u16> = "选择 rclone 引擎（rclone.exe 或官方 zip）\0"
        .encode_utf16()
        .collect();

    let mut settings: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    settings.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    settings.hwndOwner = owner as *mut std::ffi::c_void;
    settings.lpstrFilter = filter.as_ptr();
    settings.nFilterIndex = 1;
    settings.lpstrFile = buffer.as_mut_ptr();
    settings.nMaxFile = BUFFER_LEN as u32;
    settings.lpstrTitle = title.as_ptr();
    settings.Flags = OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;

    let picked = unsafe { GetOpenFileNameW(&mut settings) };
    if picked == 0 {
        // 用户点了取消：正常路径，不是错误。
        return Ok(None);
    }
    let length = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    if length == 0 {
        return Ok(None);
    }
    Ok(Some(String::from_utf16_lossy(&buffer[..length])))
}

/// 非 Windows 平台没有这个对话框。
#[cfg(not(windows))]
pub fn pick_engine_file(_owner: isize) -> Result<Option<String>, String> {
    Err("当前平台不支持系统文件选择框".into())
}
