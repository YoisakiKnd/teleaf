//! Freedesktop notifications over the user's session bus, shared by X11/Wayland.
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, mpsc::Receiver};
use std::time::Duration;

use zbus::blocking::{Proxy, connection::Builder};
use zbus::zvariant::Value;

use super::Command;

#[derive(Default)]
pub(super) struct History {
    owner: String,
    ids: VecDeque<(i64, u32)>,
}

fn body_text(text: &str, markup: bool) -> String {
    if markup {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    } else {
        text.into()
    }
}

pub(super) fn run(
    receiver: Receiver<Command>,
    _account: &str,
    history: &Arc<Mutex<History>>,
) -> Result<(), String> {
    let connection = Builder::session()
        .and_then(|builder| builder.method_timeout(Duration::from_secs(5)).build())
        .map_err(|e| format!("无法连接桌面会话 D-Bus：{e}；需要本地桌面会话"))?;
    run_connected(receiver, history, &connection)
}

fn run_connected(
    receiver: Receiver<Command>,
    history: &Arc<Mutex<History>>,
    connection: &zbus::blocking::Connection,
) -> Result<(), String> {
    let bus = Proxy::new(
        connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .map_err(|e| e.to_string())?;
    // Auto-activate the service through GetCapabilities before reading its owner.
    let proxy = Proxy::new(
        connection,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .map_err(|e| e.to_string())?;
    let _: Vec<String> = proxy
        .call("GetCapabilities", &())
        .map_err(|e| format!("桌面通知服务不可用：{e}；请启用 GNOME/KDE 通知或 dunst/mako"))?;
    let owner: String = bus
        .call("GetNameOwner", &("org.freedesktop.Notifications",))
        .map_err(|e| e.to_string())?;
    // Bind to one server instance so stale IDs cannot replace other apps after restart.
    let proxy = Proxy::new(
        connection,
        owner.as_str(),
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .map_err(|e| e.to_string())?;
    let capabilities: Vec<String> = proxy
        .call("GetCapabilities", &())
        .map_err(|e| e.to_string())?;
    let markup = capabilities.iter().any(|cap| cap == "body-markup");
    let mut history = history.lock().map_err(|e| e.to_string())?;
    // Preserve IDs when the idle worker reconnects; never reuse IDs after server restart.
    if history.owner != owner {
        history.ids.clear();
        history.owner = owner.clone();
    }
    while let Ok(command) = receiver.recv_timeout(Duration::from_secs(30)) {
        match command {
            Command::Show {
                group,
                title,
                body,
                silent,
            } => {
                let previous = history
                    .ids
                    .iter()
                    .find(|(id, _)| *id == group)
                    .map_or(0, |(_, id)| *id);
                let hints: HashMap<&str, Value<'_>> = HashMap::from([
                    ("suppress-sound", Value::from(silent)),
                    ("sound-name", Value::from("message-new-instant")),
                    ("category", Value::from("im.received")),
                ]);
                let id: u32 = proxy
                    .call(
                        "Notify",
                        &(
                            "Teleaf",
                            previous,
                            "",
                            title.as_str(),
                            body_text(&body, markup),
                            Vec::<String>::new(),
                            hints,
                            -1_i32,
                        ),
                    )
                    .map_err(|e| e.to_string())?;
                if id == 0 {
                    return Err("通知服务返回了无效的通知编号".into());
                }
                history.ids.retain(|(existing, _)| *existing != group);
                history.ids.push_back((group, id));
                while history.ids.len() > 128 {
                    if let Some((_, id)) = history.ids.pop_front() {
                        close(&proxy, id)?;
                    }
                }
            }
            Command::Remove(group) => {
                if let Some((_, id)) = history.ids.iter().find(|(existing, _)| *existing == group) {
                    close(&proxy, *id)?;
                }
                history.ids.retain(|(existing, _)| *existing != group);
            }
            Command::Clear => {
                while let Some((_, id)) = history.ids.pop_front() {
                    close(&proxy, id)?;
                }
            }
        }
    }
    Ok(())
}

fn close(proxy: &Proxy<'_>, id: u32) -> Result<(), String> {
    match proxy.call::<_, _, ()>("CloseNotification", &(id,)) {
        Ok(()) => Ok(()),
        // Expired/dismissed notifications cannot be closed again (per the spec).
        Err(zbus::Error::MethodError(name, _, _))
            if name.as_str() == "org.freedesktop.DBus.Error.InvalidArgs"
                || name.as_str() == "org.freedesktop.Notifications.Error.InvalidId" =>
        {
            Ok(())
        }
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_is_literal_text_even_on_markup_servers() {
        assert_eq!(
            body_text("<b>&你好</b>", true),
            "&lt;b&gt;&amp;你好&lt;/b&gt;"
        );
        assert_eq!(body_text("<b>&你好</b>", false), "<b>&你好</b>");
    }

    struct Bus(std::process::Child);
    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[derive(Debug, PartialEq)]
    enum Event {
        Show(u32, String, String, bool),
        Close(u32),
    }

    struct Service(std::sync::mpsc::Sender<Event>);
    #[zbus::interface(name = "org.freedesktop.Notifications")]
    impl Service {
        fn get_capabilities(&self) -> Vec<&str> {
            vec!["body-markup", "sound"]
        }
        #[allow(clippy::too_many_arguments)]
        fn notify(
            &self,
            app: &str,
            previous: u32,
            icon: &str,
            title: &str,
            body: &str,
            actions: Vec<String>,
            hints: HashMap<String, zbus::zvariant::OwnedValue>,
            timeout: i32,
        ) -> u32 {
            assert_eq!(app, "Teleaf");
            assert!(icon.is_empty() && actions.is_empty());
            assert_eq!(timeout, -1);
            let silent = bool::try_from(hints.get("suppress-sound").unwrap()).unwrap();
            self.0
                .send(Event::Show(previous, title.into(), body.into(), silent))
                .unwrap();
            if previous == 0 { 42 } else { previous }
        }
        fn close_notification(&self, id: u32) {
            self.0.send(Event::Close(id)).unwrap();
        }
    }

    #[test]
    fn private_dbus_replaces_removes_and_keeps_ids_across_worker_restarts() {
        use std::io::BufRead;
        use std::process::{Command as Process, Stdio};
        let mut bus = Bus(Process::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("Linux notification tests require dbus-daemon"));
        let mut address = String::new();
        std::io::BufReader::new(bus.0.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        let (events, observed) = std::sync::mpsc::channel();
        let server = Builder::address(address.trim())
            .unwrap()
            .serve_at("/org/freedesktop/Notifications", Service(events.clone()))
            .unwrap()
            .name("org.freedesktop.Notifications")
            .unwrap()
            .build()
            .unwrap();
        let history = Arc::new(Mutex::new(History::default()));
        for phase in 0..5 {
            let connection = Builder::address(address.trim())
                .unwrap()
                .method_timeout(Duration::from_secs(5))
                .build()
                .unwrap();
            let (sender, receiver) = std::sync::mpsc::channel();
            if phase < 2 || phase == 3 {
                sender
                    .send(Command::Show {
                        group: 7,
                        title: "群 <&>".into(),
                        body: "<b>&你好</b>".into(),
                        silent: true,
                    })
                    .unwrap();
            } else if phase == 2 {
                sender.send(Command::Remove(7)).unwrap();
            } else {
                sender.send(Command::Clear).unwrap();
            }
            drop(sender);
            run_connected(receiver, &history, &connection).unwrap();
            let expected = if phase < 2 || phase == 3 {
                Event::Show(
                    if phase == 0 || phase == 3 { 0 } else { 42 },
                    "群 <&>".into(),
                    "&lt;b&gt;&amp;你好&lt;/b&gt;".into(),
                    true,
                )
            } else {
                Event::Close(42)
            };
            assert_eq!(
                observed.recv_timeout(Duration::from_secs(1)).unwrap(),
                expected
            );
        }
        assert!(history.lock().unwrap().ids.is_empty());
        history.lock().unwrap().ids.push_back((7, 99));
        server.close().unwrap();
        let _new_server = Builder::address(address.trim())
            .unwrap()
            .serve_at("/org/freedesktop/Notifications", Service(events))
            .unwrap()
            .name("org.freedesktop.Notifications")
            .unwrap()
            .build()
            .unwrap();
        let connection = Builder::address(address.trim())
            .unwrap()
            .method_timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(Command::Show {
                group: 7,
                title: "新服务".into(),
                body: "纯文字".into(),
                silent: false,
            })
            .unwrap();
        drop(sender);
        run_connected(receiver, &history, &connection).unwrap();
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(1)).unwrap(),
            Event::Show(0, "新服务".into(), "纯文字".into(), false)
        );
    }
}
