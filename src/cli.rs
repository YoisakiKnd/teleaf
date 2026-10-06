//! Installation checks run before terminal mode or account configuration is opened.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Run { demo: bool },
    Help,
    Version,
    Check,
    DismissNotification,
    TestNotification,
}

pub const HELP: &str = "Teleaf — Telegram 终端客户端

用法：teleaf [选项]
  --demo       离线演示，不读取账号配置
  --check      检查随附 TDLib 能否加载，不登录或打开数据库
  --test-notification  测试系统桌面通知，无需登录
  --version    显示版本
  --help       显示帮助

直接运行 teleaf 开始登录；可用 TG_DATA_DIR 指定账号数据目录，
TDLIB_PATH 指定自备 TDLib。界面内按 ? 查看快捷键。";

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let args: Vec<_> = args.into_iter().collect();
    match args.as_slice() {
        [] => Ok(Command::Run { demo: false }),
        [arg] => match arg.as_str() {
            "--demo" => Ok(Command::Run { demo: true }),
            "--help" | "-h" => Ok(Command::Help),
            "--version" | "-V" => Ok(Command::Version),
            "--check" => Ok(Command::Check),
            // A fixed Windows toast protocol handler; never opens a second TDLib session.
            "--dismiss-notification" => Ok(Command::DismissNotification),
            "--test-notification" => Ok(Command::TestNotification),
            _ => Err(format!("未知参数：{arg}；运行 teleaf --help 查看用法")),
        },
        _ => Err("每次只支持一个选项；运行 teleaf --help 查看用法".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_commands_and_unknown_options() {
        for (args, expected) in [
            (vec![], Command::Run { demo: false }),
            (vec!["--demo"], Command::Run { demo: true }),
            (vec!["--check"], Command::Check),
            (vec!["--version"], Command::Version),
            (vec!["--help"], Command::Help),
            (vec!["--dismiss-notification"], Command::DismissNotification),
            (vec!["--test-notification"], Command::TestNotification),
        ] {
            assert_eq!(parse(args.into_iter().map(str::to_owned)), Ok(expected));
        }
        assert!(parse(["--bad".into()]).is_err());
        assert!(parse(["--demo".into(), "--check".into()]).is_err());
    }
}
