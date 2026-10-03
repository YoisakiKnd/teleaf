//! Minimal TDLib JSON bridge. Only one worker calls `td_receive`, preserving update order.

use std::ffi::{CStr, CString, c_char, c_double, c_int};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::thread;
use std::time::{Duration, Instant};

use libloading::Library;
use serde_json::{Value, json};

type CreateClientId = unsafe extern "C" fn() -> c_int;
type Send = unsafe extern "C" fn(c_int, *const c_char);
type Receive = unsafe extern "C" fn(c_double) -> *const c_char;
type Execute = unsafe extern "C" fn(*const c_char) -> *const c_char;

struct TdJson {
    _library: Library,
    client_id: c_int,
    path: PathBuf,
    version: String,
    send: Send,
    receive: Receive,
}

impl TdJson {
    fn load() -> Result<Self, String> {
        let path = library_path();
        // SAFETY: The library is held for the lifetime of all copied symbols.
        let library = unsafe { load_library(&path) }.map_err(|error| {
            format!(
                "无法加载 TDLib（{}）：{error}；预编译版请重新安装完整包，源码版运行 python3 scripts/install-tdlib.py，或设置 TDLIB_PATH",
                path.display()
            )
        })?;
        // SAFETY: These signatures are defined by TDLib's td_json_client.h C interface.
        let (create, send, receive, execute) = unsafe {
            let create = *library
                .get::<CreateClientId>(b"td_create_client_id\0")
                .map_err(|error| format!("TDLib 缺少 td_create_client_id：{error}"))?;
            let send = *library
                .get::<Send>(b"td_send\0")
                .map_err(|error| format!("TDLib 缺少 td_send：{error}"))?;
            let receive = *library
                .get::<Receive>(b"td_receive\0")
                .map_err(|error| format!("TDLib 缺少 td_receive：{error}"))?;
            let execute = *library
                .get::<Execute>(b"td_execute\0")
                .map_err(|error| format!("TDLib 缺少 td_execute：{error}"))?;
            (create, send, receive, execute)
        };
        // TDLib logs to stderr by default, which corrupts the alternate-screen UI.
        // Configure logging before creating the client, as in TDLib's JSON example.
        let log_request = c"{\"@type\":\"setLogVerbosityLevel\",\"new_verbosity_level\":0}";
        // SAFETY: `execute` is a valid symbol and the static request is NUL-terminated.
        let log_response = unsafe { execute(log_request.as_ptr()) };
        if log_response.is_null() {
            return Err("TDLib 未能关闭终端日志".into());
        }
        // SAFETY: TDLib keeps this response valid until its next execute/receive call.
        let log_response = unsafe { CStr::from_ptr(log_response) };
        let log_response: Value = serde_json::from_slice(log_response.to_bytes())
            .map_err(|error| format!("TDLib 日志配置返回无效 JSON：{error}"))?;
        if log_response.get("@type").and_then(Value::as_str) != Some("ok") {
            return Err(format!("TDLib 未能关闭终端日志：{log_response}"));
        }
        let version_request = c"{\"@type\":\"getOption\",\"name\":\"version\"}";
        // SAFETY: The static request is NUL-terminated and the symbol is valid.
        let version_response = unsafe { execute(version_request.as_ptr()) };
        if version_response.is_null() {
            return Err("TDLib 未返回版本号".into());
        }
        // SAFETY: Copy the returned value before another TDLib call invalidates it.
        let version_response = unsafe { CStr::from_ptr(version_response) };
        let version_response: Value = serde_json::from_slice(version_response.to_bytes())
            .map_err(|error| format!("TDLib 版本信息不是有效 JSON：{error}"))?;
        let version = version_response
            .get("value")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("TDLib 未返回版本号：{version_response}"))?
            .to_owned();
        // SAFETY: `create` is a valid loaded function pointer with no arguments.
        let client_id = unsafe { create() };
        let client = Self {
            _library: library,
            client_id,
            path,
            version,
            send,
            receive,
        };
        // A new TDLib client emits no authorization updates before its first request.
        client.send(&json!({"@type": "getOption", "name": "version"}))?;
        Ok(client)
    }

    fn send(&self, value: &Value) -> Result<(), String> {
        let payload = CString::new(value.to_string())
            .map_err(|_| "TDLib 请求含有非法 NUL 字符".to_owned())?;
        // SAFETY: The payload remains valid for the duration of `td_send`, which copies it.
        unsafe { (self.send)(self.client_id, payload.as_ptr()) };
        Ok(())
    }

    fn receive(&self) -> Result<Option<Value>, String> {
        // SAFETY: This worker is the only thread calling `td_receive`. The returned
        // pointer is copied before the next call, as required by TDLib.
        let result = unsafe { (self.receive)(0.1) };
        if result.is_null() {
            return Ok(None);
        }
        // SAFETY: TDLib returns a NUL-terminated JSON string valid until next receive.
        let bytes = unsafe { CStr::from_ptr(result) }.to_bytes();
        serde_json::from_slice(bytes)
            .map(Some)
            .map_err(|error| format!("TDLib 返回无效 JSON：{error}"))
    }
}

unsafe fn load_library(path: &std::path::Path) -> Result<Library, libloading::Error> {
    #[cfg(windows)]
    if let Ok(absolute) = path.canonicalize() {
        // SAFETY: Dependency lookup includes the DLL's own directory and standard
        // loader locations, so bundled OpenSSL/zlib do not depend on the user's cwd.
        return unsafe {
            libloading::os::windows::Library::load_with_flags(
                absolute,
                libloading::os::windows::LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR
                    | libloading::os::windows::LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
            .map(Into::into)
        };
    }
    // SAFETY: Caller keeps the library alive while using its symbols.
    unsafe { Library::new(path) }
}

pub fn runtime_info() -> Result<(String, PathBuf), String> {
    let path = library_path();
    // SAFETY: The handle is retained until all symbol calls and JSON copies finish.
    let library = unsafe { load_library(&path) }
        .map_err(|error| format!("无法加载 TDLib（{}）：{error}", path.display()))?;
    // SAFETY: Validate the expected TDLib C API without creating a client or database.
    unsafe {
        for symbol in [
            b"td_create_client_id\0".as_slice(),
            b"td_send\0",
            b"td_receive\0",
        ] {
            library
                .get::<*const ()>(symbol)
                .map_err(|e| e.to_string())?;
        }
        let execute = library
            .get::<Execute>(b"td_execute\0")
            .map_err(|e| e.to_string())?;
        let result = execute(c"{\"@type\":\"getOption\",\"name\":\"version\"}".as_ptr());
        if result.is_null() {
            return Err("TDLib 未返回版本号".into());
        }
        let value: Value =
            serde_json::from_slice(CStr::from_ptr(result).to_bytes()).map_err(|e| e.to_string())?;
        let version = value["value"].as_str().ok_or("TDLib 未返回版本号")?;
        Ok((version.into(), path))
    }
}

fn library_path() -> PathBuf {
    if let Some(path) = std::env::var_os("TDLIB_PATH").filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    let name = if cfg!(target_os = "macos") {
        "libtdjson.dylib"
    } else if cfg!(target_os = "windows") {
        "tdjson.dll"
    } else {
        "libtdjson.so"
    };
    let beside_executable = std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok())
        .and_then(|path| path.parent().map(|parent| parent.join("tdlib").join(name)));
    let project_install = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("tdlib")
        .join(name);
    beside_executable
        .filter(|path| path.is_file())
        .or_else(|| project_install.is_file().then_some(project_install))
        .unwrap_or_else(|| PathBuf::from(name))
}

pub enum TdEvent {
    Connected { version: String, path: PathBuf },
    Error(String),
    Update(Value),
}

pub(crate) enum TdCommand {
    Request(Value),
    Shutdown,
}

pub struct TdWorker {
    pub events: Receiver<TdEvent>,
    commands: SyncSender<TdCommand>,
    join: Option<thread::JoinHandle<()>>,
}

impl TdWorker {
    #[cfg(test)]
    pub(crate) fn test_pair() -> (Self, Receiver<TdCommand>) {
        let (commands, requests) = mpsc::sync_channel(128);
        let (_, events) = mpsc::sync_channel(1);
        (
            Self {
                events,
                commands,
                join: None,
            },
            requests,
        )
    }

    pub fn spawn_demo() -> Self {
        let (event_sender, events) = mpsc::sync_channel(64);
        let (commands, receiver) = mpsc::sync_channel(128);
        let join = thread::spawn(move || crate::demo::serve(event_sender, receiver));
        Self {
            events,
            commands,
            join: Some(join),
        }
    }

    pub fn spawn() -> Self {
        // Backpressure caps memory use if the UI falls behind incoming updates.
        let (event_sender, events) = mpsc::sync_channel(64);
        let (commands, command_receiver) = mpsc::sync_channel(128);
        let join = thread::spawn(move || {
            let client = match TdJson::load() {
                Ok(client) => client,
                Err(error) => {
                    let _ = event_sender.send(TdEvent::Error(error));
                    return;
                }
            };
            if event_sender
                .send(TdEvent::Connected {
                    version: client.version.clone(),
                    path: client.path.clone(),
                })
                .is_err()
            {
                close_client(&client);
                return;
            }

            loop {
                loop {
                    match command_receiver.try_recv() {
                        Ok(TdCommand::Request(value)) => {
                            if let Err(error) = client.send(&value) {
                                let _ = event_sender.send(TdEvent::Error(error));
                            }
                        }
                        Ok(TdCommand::Shutdown) | Err(TryRecvError::Disconnected) => {
                            close_client(&client);
                            return;
                        }
                        Err(TryRecvError::Empty) => break,
                    }
                }
                match client.receive() {
                    Ok(Some(update)) => {
                        if event_sender.send(TdEvent::Update(update)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        let _ = event_sender.send(TdEvent::Error(error));
                        break;
                    }
                }
            }
            close_client(&client);
        });

        Self {
            events,
            commands,
            join: Some(join),
        }
    }

    pub fn request(&self, value: Value) -> Result<(), String> {
        self.commands
            .try_send(TdCommand::Request(value))
            .map_err(|error| match error {
                TrySendError::Full(_) => "TDLib 请求队列已满；请稍后重试".to_owned(),
                TrySendError::Disconnected(_) => "TDLib 工作线程已停止".to_owned(),
            })
    }

    pub fn shutdown(mut self) {
        // Release a producer blocked on the bounded event channel.
        drop(self.events);
        let _ = self.commands.try_send(TdCommand::Shutdown);
        // If the queue is full, disconnecting it still makes the worker exit
        // once it has drained the finite queue.
        drop(self.commands);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn close_client(client: &TdJson) {
    let _ = client.send(&json!({"@type": "close"}));
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        match client.receive() {
            Ok(Some(value))
                if value
                    .pointer("/authorization_state/@type")
                    .and_then(Value::as_str)
                    == Some("authorizationStateClosed") =>
            {
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
}
