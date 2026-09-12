//! Windows 计划任务：自启的动作载体。
//!
//! 三个必须显式设置的值（旧版实测，缺一个就出难查的故障）：
//! - `ExecutionTimeLimit=PT0S`：否则默认 3 天后任务被强制结束，盘符静默消失；
//! - `DisallowStartIfOnBatteries=false`：否则笔记本拔电源后不启动；
//! - `MultipleInstancesPolicy=IgnoreNew`：否则可能拉起第二个实例，端口冲突。
//!
//! 动作参数由 `quote::command_line` 序列化：含空格路径必须逐个加引号。

use std::path::{Path, PathBuf};

use foundation_core::{FoundationError, Result};

use crate::quote;

pub const TASK_NAME_BOOT: &str = "WebDavDrive-Agent";
pub const TASK_NAME_LOGON: &str = "WebDavDrive-Agent-User";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskMode {
    /// 开机、SYSTEM、盘符全局可见。
    Boot,
    /// 用户登录、当前账户、盘符仅会话可见（默认推荐）。
    Logon,
}

impl TaskMode {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "boot" => Ok(Self::Boot),
            "logon" => Ok(Self::Logon),
            other => Err(FoundationError::InvalidInput(format!(
                "未知的自启模式 {other:?}（只支持 logon / boot）"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::Logon => "logon",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TaskSpec {
    pub name: String,
    pub executable: PathBuf,
    pub arguments: String,
    pub working_dir: PathBuf,
    pub mode: TaskMode,
    /// logon 模式使用的账户（DOMAIN\User）；boot 模式忽略。
    pub account: String,
}

impl TaskSpec {
    pub fn boot(
        executable: impl Into<PathBuf>,
        args: &[String],
        working_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            name: TASK_NAME_BOOT.to_string(),
            executable: executable.into(),
            arguments: quote::command_line(args),
            working_dir: working_dir.into(),
            mode: TaskMode::Boot,
            account: String::new(),
        }
    }

    pub fn logon(
        executable: impl Into<PathBuf>,
        args: &[String],
        working_dir: impl Into<PathBuf>,
        account: impl Into<String>,
    ) -> Self {
        Self {
            name: TASK_NAME_LOGON.to_string(),
            executable: executable.into(),
            arguments: quote::command_line(args),
            working_dir: working_dir.into(),
            mode: TaskMode::Logon,
            account: account.into(),
        }
    }
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// 生成任务 XML（纯函数，便于测试；注册前必须转成 UTF-16LE+BOM）。
pub fn render_xml(spec: &TaskSpec) -> String {
    let (trigger, principal, extra_settings) = match spec.mode {
        TaskMode::Boot => (
            "<BootTrigger><Enabled>true</Enabled><Delay>PT20S</Delay></BootTrigger>".to_string(),
            r#"<Principal id="Author"><UserId>S-1-5-18</UserId><RunLevel>HighestAvailable</RunLevel></Principal>"#
                .to_string(),
            String::new(),
        ),
        TaskMode::Logon => (
            format!(
                "<LogonTrigger><Enabled>true</Enabled><UserId>{}</UserId><Delay>PT10S</Delay></LogonTrigger>",
                escape(&spec.account)
            ),
            format!(
                r#"<Principal id="Author"><UserId>{}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal>"#,
                escape(&spec.account)
            ),
            String::new(),
        ),
    };
    let _ = extra_settings;

    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>WebDAV Drive: starts the local agent and mounts every profile marked 开机自动挂载.</Description>
    <URI>\{name}</URI>
  </RegistrationInfo>
  <Triggers>{trigger}</Triggers>
  <Principals>{principal}</Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <RestartOnFailure><Interval>PT1M</Interval><Count>3</Count></RestartOnFailure>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>{arguments}</Arguments>
      <WorkingDirectory>{working_dir}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#,
        name = escape(&spec.name),
        trigger = trigger,
        principal = principal,
        command = escape(&spec.executable.to_string_lossy()),
        arguments = escape(&spec.arguments),
        working_dir = escape(&spec.working_dir.to_string_lossy()),
    )
}

/// 注册/覆盖计划任务。boot 模式需要管理员权限。
pub fn install(spec: &TaskSpec) -> Result<()> {
    #[cfg(windows)]
    {
        let xml = render_xml(spec);
        let xml_path = std::env::temp_dir().join(format!("{}.xml", spec.name));
        write_utf16_with_bom(&xml_path, &xml)?;

        let output = std::process::Command::new("schtasks")
            .args(["/Create", "/TN", &spec.name, "/XML"])
            .arg(&xml_path)
            .arg("/F")
            .output()
            .map_err(|err| FoundationError::Platform(format!("无法执行 schtasks：{err}")))?;
        let _ = std::fs::remove_file(&xml_path);

        if !output.status.success() {
            return Err(FoundationError::Platform(format!(
                "注册计划任务失败（模式 {}）：{} {}",
                spec.mode.as_str(),
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        log::info!("已注册计划任务 {}（{}）", spec.name, spec.mode.as_str());
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = spec;
        Err(FoundationError::Platform(
            "计划任务仅在 Windows 上可用".into(),
        ))
    }
}

/// 删除任务；不存在视为成功。
pub fn uninstall(name: &str) -> Result<()> {
    #[cfg(windows)]
    {
        let output = std::process::Command::new("schtasks")
            .args(["/Delete", "/TN", name, "/F"])
            .output()
            .map_err(|err| FoundationError::Platform(format!("无法执行 schtasks：{err}")))?;
        if output.status.success() || !status(name)? {
            return Ok(());
        }
        Err(FoundationError::Platform(format!(
            "删除计划任务 {name} 失败：{}",
            String::from_utf8_lossy(&output.stdout).trim()
        )))
    }
    #[cfg(not(windows))]
    {
        let _ = name;
        Err(FoundationError::Platform(
            "计划任务仅在 Windows 上可用".into(),
        ))
    }
}

/// 任务是否已注册。
pub fn status(name: &str) -> Result<bool> {
    #[cfg(windows)]
    {
        let output = std::process::Command::new("schtasks")
            .args(["/Query", "/TN", name])
            .output()
            .map_err(|err| FoundationError::Platform(format!("无法执行 schtasks：{err}")))?;
        Ok(output.status.success())
    }
    #[cfg(not(windows))]
    {
        let _ = name;
        Err(FoundationError::Platform(
            "计划任务仅在 Windows 上可用".into(),
        ))
    }
}

/// schtasks /XML 要求 UTF-16 编码（带 BOM）。
pub fn write_utf16_with_bom(path: &Path, text: &str) -> Result<()> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boot_spec() -> TaskSpec {
        TaskSpec::boot(
            r"C:\py\pythonw.exe",
            &[
                "--service".into(),
                "--data-dir".into(),
                r"C:\My Data".into(),
            ],
            r"C:\app",
        )
    }

    #[test]
    fn boot_xml_has_required_settings() {
        let spec = boot_spec();
        // 引号在命令行层完成；进入 XML 后会被实体转义，Task Scheduler 解析时还原
        assert_eq!(spec.arguments, r#"--service --data-dir "C:\My Data""#);
        let xml = render_xml(&spec);
        assert!(xml.contains("<BootTrigger>"));
        assert!(xml.contains("S-1-5-18"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(xml.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
        assert!(
            xml.contains("&quot;C:"),
            "含空格参数在 XML 中必须带引号实体：{xml}"
        );
    }

    #[test]
    fn logon_xml_uses_interactive_token_and_account() {
        let mut spec = boot_spec();
        spec.mode = TaskMode::Logon;
        spec.account = r"PC\alice".into();
        let xml = render_xml(&spec);
        assert!(xml.contains("<LogonTrigger>"));
        assert!(xml.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(xml.contains(r"PC\alice"));
    }

    #[test]
    fn xml_escapes_injection_in_paths() {
        let spec = TaskSpec::boot(r"C:\a&b\<evil>.exe", &["--flag".into()], r#"C:\dir"quote"#);
        let xml = render_xml(&spec);
        assert!(xml.contains("&amp;"));
        assert!(xml.contains("&lt;evil&gt;"));
        assert!(!xml.contains("<evil>"));
    }

    #[test]
    fn unknown_mode_is_rejected() {
        assert!(TaskMode::parse("boot-but-typo").is_err());
        assert_eq!(TaskMode::parse("logon").unwrap().as_str(), "logon");
    }
}
