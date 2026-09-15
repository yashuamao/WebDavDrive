//! Windows 命令行引号。
//!
//! Task Scheduler 的 `<Arguments>` 是一个字符串，由系统按标准 Windows 规则拆分；
//! 用 `" ".join(args)` 会把 `--data-dir C:\My Data` 拆成两个参数，并且宿主 stderr
//! 通常无处可看，表现为"静默不启动"。这里实现 MSVCRT/CommandLineToArgvW 规则。

/// 判断是否需要加引号：空串、含空白或双引号时必须。
fn needs_quoting(arg: &str) -> bool {
    arg.is_empty()
        || arg
            .chars()
            .any(|c| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '"'))
}

/// 把单个参数序列化为 Windows 命令行片段。
pub fn quote_arg(arg: &str) -> String {
    if !needs_quoting(arg) {
        return arg.to_string();
    }

    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut backslashes = 0usize;
    for ch in arg.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                // 反斜杠只有在双引号前才需要加倍；引号本身要转义
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                if backslashes > 0 {
                    out.push_str(&"\\".repeat(backslashes));
                    backslashes = 0;
                }
                out.push(ch);
            }
        }
    }
    // 结尾的反斜杠会被解释为转义引号，必须加倍
    if backslashes > 0 {
        out.push_str(&"\\".repeat(backslashes * 2));
    }
    out.push('"');
    out
}

/// 把参数列表拼成一条命令行。
pub fn command_line<I, S>(args: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .map(|arg| quote_arg(arg.as_ref()))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_args_stay_plain() {
        assert_eq!(quote_arg("--service"), "--service");
        assert_eq!(quote_arg(r"C:\app\agent.exe"), r"C:\app\agent.exe");
    }

    #[test]
    fn spaces_get_quoted() {
        assert_eq!(quote_arg(r"C:\My Data"), r#""C:\My Data""#);
        assert_eq!(
            command_line(["--data-dir", r"C:\My Data\AgentData"]),
            r#"--data-dir "C:\My Data\AgentData""#
        );
    }

    #[test]
    fn empty_arg_is_quoted() {
        assert_eq!(quote_arg(""), r#""""#);
    }

    #[test]
    fn quotes_are_escaped() {
        assert_eq!(quote_arg(r#"a"b"#), r#""a\"b""#);
    }

    #[test]
    fn trailing_backslashes_are_doubled_inside_quotes() {
        assert_eq!(quote_arg(r"C:\dir with space\"), r#""C:\dir with space\\""#);
    }
}
