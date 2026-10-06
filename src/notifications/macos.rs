//! Keep AppKit/UserNotifications out of the TUI's address space.
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command as Process, Stdio};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use super::Command;
use serde_json::{Value, json};

fn helper() -> Result<PathBuf, String> {
    let binary = std::env::current_exe().map_err(|e| e.to_string())?;
    let path = binary
        .parent()
        .ok_or("无法定位程序目录")?
        .join("Teleaf Notifications.app/Contents/MacOS/teleaf-notifications");
    if !path.is_file() {
        return Err("缺少 Teleaf Notifications.app；请保留完整安装包或重新 cargo build".into());
    }
    Ok(path)
}

fn payload(command: Command) -> Value {
    match command {
        Command::Show {
            group,
            title,
            body,
            silent,
        } => json!({
            "op":"show", "group":group, "title":title, "body":body, "silent":silent,
        }),
        Command::Remove(group) => json!({"op":"remove", "group":group}),
        Command::Clear => json!({"op":"clear"}),
    }
}

fn apply(path: &std::path::Path, account: &str, commands: Vec<Command>) -> Result<(), String> {
    let data = serde_json::to_vec(&json!({
        "account":account, "commands":commands.into_iter().map(payload).collect::<Vec<_>>(),
    }))
    .map_err(|e| e.to_string())?;
    if data.len() > 512 * 1024 {
        return Err("通知批次超过大小限制".into());
    }
    let mut child = Process::new(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("无法启动通知助手：{e}"))?;
    let result = (|| {
        // Send message data only over stdin; never shell-evaluate or expose it in argv.
        child
            .stdin
            .take()
            .ok_or("无法打开通知助手输入")?
            .write_all(&data)
            .map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(65);
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                let mut output = String::new();
                child
                    .stdout
                    .take()
                    .ok_or("无法读取通知助手结果")?
                    .take(8192)
                    .read_to_string(&mut output)
                    .map_err(|e| e.to_string())?;
                let reply: Value = serde_json::from_str(&output)
                    .map_err(|_| "通知助手没有返回有效结果；请检查 macOS 通知权限".to_string())?;
                if status.success() && reply["ok"] == true {
                    return Ok(());
                }
                return Err(reply["error"].as_str().unwrap_or("通知助手失败").into());
            }
            if Instant::now() >= deadline {
                return Err("macOS 通知助手响应超时".into());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

pub(super) fn run(receiver: Receiver<Command>, account: &str) -> Result<(), String> {
    let path = helper()?;
    while let Ok(first) = receiver.recv_timeout(Duration::from_secs(30)) {
        let mut batch = vec![first];
        // Coalesce queued updates to the same group before launching one short-lived helper.
        while batch.len() < 16 {
            let Ok(next) = receiver.try_recv() else {
                break;
            };
            let group = match &next {
                Command::Show { group, .. } | Command::Remove(group) => Some(*group),
                Command::Clear => None,
            };
            if let Some(group) = group {
                batch.retain(|command| match command {
                    Command::Show {
                        group: previous, ..
                    }
                    | Command::Remove(previous) => *previous != group,
                    Command::Clear => true,
                });
            } else {
                batch.clear();
            }
            batch.push(next);
        }
        apply(&path, account, batch)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_helper_accepts_native_text_without_permission_or_notifications() {
        // Test executables live in deps/, while the helper is beside the main binary.
        let path = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("Teleaf Notifications.app/Contents/MacOS/teleaf-notifications");
        let result = Process::new(path).arg("--self-test").output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&result.stdout).unwrap()["ok"],
            true
        );
        let text = payload(Command::Show {
            group: 1,
            title: "<&>".into(),
            body: "$(not-code)".into(),
            silent: true,
        });
        assert_eq!(text["body"], "$(not-code)");
        assert_eq!(text["silent"], true);
    }
}
