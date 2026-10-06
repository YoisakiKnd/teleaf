//! WinRT toast backend. All COM objects stay on this worker thread.
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{
    NotificationSetting, ToastNotification, ToastNotificationManager,
};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::StructuredStorage::{
    InitPropVariantFromCLSID, PROPVARIANT, PVCHF_DEFAULT, PropVariantChangeType,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, IPersistFile,
};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RegCloseKey, RegCreateKeyW, RegSetValueExW,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    FOLDERID_Programs, IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
    SetCurrentProcessExplicitAppUserModelID, ShellLink,
};
use windows::core::{GUID, HSTRING, Interface, PCWSTR};

use super::Command;

const APP_ID: &str = "org.teleaf.client";
const APP_ID_KEY: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    pid: 5,
};
const ACTIVATOR_KEY: PROPERTYKEY = PROPERTYKEY {
    fmtid: APP_ID_KEY.fmtid,
    pid: 26,
};
const STUB_CLSID: GUID = GUID::from_u128(0x3bb04cc8_54d8_4bc0_9798_0d0397bb7556);

struct Apartment;
impl Apartment {
    fn new() -> windows::core::Result<Self> {
        // SAFETY: This dedicated thread has not initialized COM elsewhere.
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED)?;
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: Balances this thread's successful RoInitialize, after COM objects drop.
        unsafe {
            RoUninitialize();
        }
    }
}

fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn shortcut(path: &Path, executable: &Path, app_id: &str) -> windows::core::Result<()> {
    let executable = wide(executable);
    let path = wide(path);
    // SAFETY: COM is initialized; all strings remain valid through each copying call.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(PCWSTR(executable.as_ptr()))?;
        link.SetDescription(windows::core::w!("Teleaf Telegram terminal client"))?;
        let properties: IPropertyStore = link.cast()?;
        // PROPVARIANT has an automatic destructor in windows-rs. Convert an owned
        // BSTR to an owned LPWSTR, so every success/error path frees system memory.
        let mut value = PROPVARIANT::default();
        PropVariantChangeType(
            &mut value,
            &PROPVARIANT::from(app_id),
            PVCHF_DEFAULT,
            VT_LPWSTR,
        )?;
        properties.SetValue(&APP_ID_KEY, &value)?;
        // Microsoft-supported stub CLSID + protocol activation persists unpackaged toasts.
        let activator = InitPropVariantFromCLSID(&STUB_CLSID)?;
        properties.SetValue(&ACTIVATOR_KEY, &activator)?;
        properties.Commit()?;
        let file: IPersistFile = link.cast()?;
        file.Save(PCWSTR(path.as_ptr()), true)?;
    }
    Ok(())
}

fn register() -> Result<(), String> {
    // SAFETY: Windows allocates the known-folder path; always release it after copying.
    let folder = unsafe {
        let folder = SHGetKnownFolderPath(&FOLDERID_Programs, KF_FLAG_DEFAULT, None)
            .map_err(|e| format!("读取当前用户开始菜单目录失败：{e}"))?;
        let copied = folder.to_string();
        CoTaskMemFree(Some(folder.0.cast()));
        copied.map_err(|e| e.to_string())?
    };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    register_identity(APP_ID)?;
    let app_id = HSTRING::from(APP_ID);
    // SAFETY: applies to this process only; the copied identifier is stable and
    // does not change Windows Terminal's or Scoop's launcher identity.
    unsafe { SetCurrentProcessExplicitAppUserModelID(PCWSTR(app_id.as_ptr())) }
        .map_err(|e| format!("设置 Windows 通知应用标识失败：{e}"))?;
    let command = format!(
        "\"{}\" --dismiss-notification",
        executable
            .to_str()
            .ok_or("通知处理程序路径不是有效 UTF-8")?
    );
    registry_string(
        r"Software\Classes\teleaf-notification",
        "",
        "URL:Teleaf Notifications",
    )?;
    registry_string(r"Software\Classes\teleaf-notification", "URL Protocol", "")?;
    registry_string(
        r"Software\Classes\teleaf-notification\shell\open\command",
        "",
        &command,
    )?;
    // Refresh only our own notification identity; Scoop's launcher is untouched.
    shortcut(
        &folder.join("Teleaf Notifications.lnk"),
        &executable,
        APP_ID,
    )
    .map_err(|e| format!("保存通知开始菜单快捷方式失败：{e}"))
}

fn identity_key(app_id: &str) -> String {
    format!(r"Software\Classes\AppUserModelId\{app_id}")
}

fn register_identity(app_id: &str) -> Result<(), String> {
    // Register synchronously before querying Setting(). Start-menu indexing is
    // asynchronous and a shortcut alone may not yet resolve on a fresh install.
    // This is per-user metadata, as used by Microsoft's notification toolkit.
    let key = identity_key(app_id);
    registry_string(&key, "DisplayName", "Teleaf Notifications")?;
    registry_string(&key, "CustomActivator", &format!("{{{STUB_CLSID:?}}}"))
}

fn registry_string(path: &str, name: &str, value: &str) -> Result<(), String> {
    let path = HSTRING::from(path);
    let name = HSTRING::from(name);
    let data: Vec<u8> = value
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    // SAFETY: NUL-terminated strings and byte slices live through the copying calls.
    unsafe {
        let mut key = windows::Win32::System::Registry::HKEY::default();
        RegCreateKeyW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()), &mut key)
            .ok()
            .map_err(|e| format!("创建 Windows 通知注册项失败（{path}）：{e}"))?;
        let result = RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&data)).ok();
        let _ = RegCloseKey(key);
        result.map_err(|e| format!("写入 Windows 通知注册项失败（{path} / {name}）：{e}"))
    }
}

fn xml(title: &str, body: &str, silent: bool) -> String {
    fn escape(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
    format!(
        "<toast duration=\"short\" activationType=\"protocol\" launch=\"teleaf-notification://dismiss\"><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual>{}</toast>",
        escape(title),
        escape(body),
        if silent {
            "<audio silent=\"true\"/>"
        } else {
            ""
        }
    )
}

pub(super) fn run(receiver: Receiver<Command>, account: &str) -> Result<(), String> {
    let _apartment = Apartment::new().map_err(|e| format!("初始化 Windows 通知组件失败：{e}"))?;
    register()?;
    let app_id = HSTRING::from(APP_ID);
    let account = HSTRING::from(account);
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&app_id)
        .map_err(|e| format!("创建 Windows 通知发送器失败（{APP_ID}）：{e}"))?;
    let setting = notifier
        .Setting()
        .map_err(|e| format!("读取 Windows 通知设置失败（{APP_ID}）：{e}"))?;
    if setting != NotificationSetting::Enabled {
        return Err(format!(
            "Windows 禁止 Teleaf Notifications 显示通知（{}）；请检查系统通知设置或组策略",
            setting_name(setting)
        ));
    }
    // An idle worker exits; the next notification creates one again, with no timer in the UI.
    while let Ok(command) = receiver.recv_timeout(Duration::from_secs(30)) {
        let (stage, result) = match command {
            Command::Show {
                group,
                title,
                body,
                silent,
            } => (
                "发送 Windows 通知",
                (|| {
                    let document = XmlDocument::new()?;
                    document.LoadXml(&HSTRING::from(xml(&title, &body, silent)))?;
                    let toast = ToastNotification::CreateToastNotification(&document)?;
                    // One toast per Telegram group; new messages replace it in Action Center.
                    toast.SetTag(&HSTRING::from(group.to_string()))?;
                    toast.SetGroup(&account)?;
                    notifier.Show(&toast)
                })(),
            ),
            Command::Remove(group) => (
                "撤回 Windows 通知",
                ToastNotificationManager::History().and_then(|history| {
                    missing_history_is_ok(history.RemoveGroupedTagWithId(
                        &HSTRING::from(group.to_string()),
                        &account,
                        &app_id,
                    ))
                }),
            ),
            Command::Clear => (
                "清理 Windows 通知",
                ToastNotificationManager::History().and_then(|history| {
                    missing_history_is_ok(history.RemoveGroupWithId(&account, &app_id))
                }),
            ),
        };
        result.map_err(|e| format!("{stage}失败：{e}"))?;
    }
    Ok(())
}

fn setting_name(setting: NotificationSetting) -> &'static str {
    match setting {
        NotificationSetting::Enabled => "已开启",
        NotificationSetting::DisabledForApplication => "应用通知已关闭",
        NotificationSetting::DisabledForUser => "当前用户的系统通知已关闭",
        NotificationSetting::DisabledByGroupPolicy => "组策略禁止通知",
        NotificationSetting::DisabledByManifest => "应用声明不允许通知",
        _ => "未知系统状态",
    }
}

fn missing_history_is_ok(result: windows::core::Result<()>) -> windows::core::Result<()> {
    match result {
        // Removing a toast already dismissed/read (or clearing a fresh install)
        // must not permanently disable the notification worker.
        Err(error) if error.code().0 as u32 == 0x80070490 => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_notification_identity_resolves_before_any_toast_or_start_menu_indexing() {
        use windows::Win32::System::Registry::{RRF_RT_REG_SZ, RegDeleteTreeW, RegGetValueW};
        let _apartment = Apartment::new().unwrap();
        let mut nonce = [0u8; 16];
        getrandom::getrandom(&mut nonce).unwrap();
        let nonce: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
        let app_id = format!("org.teleaf.registration-test.{nonce}");
        let key = HSTRING::from(identity_key(&app_id));
        struct Cleanup(HSTRING);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                // SAFETY: removes only this test's freshly generated per-user key.
                unsafe {
                    let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(self.0.as_ptr()));
                }
            }
        }
        let _cleanup = Cleanup(key.clone());
        let before = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(&app_id))
            .and_then(|notifier| notifier.Setting());
        println!("Windows unregistered notification identity: {before:?}");
        register_identity(&app_id).unwrap();
        let read = |name: &str| {
            let name = HSTRING::from(name);
            let mut buffer = [0u16; 128];
            let mut size = std::mem::size_of_val(&buffer) as u32;
            // SAFETY: writable buffer/count have the declared lengths. Read only
            // this test key and require REG_SZ, including its terminating NUL.
            unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    PCWSTR(key.as_ptr()),
                    PCWSTR(name.as_ptr()),
                    RRF_RT_REG_SZ,
                    None,
                    Some(buffer.as_mut_ptr().cast()),
                    Some(&mut size),
                )
                .ok()
                .unwrap();
            }
            String::from_utf16(&buffer[..size as usize / 2 - 1]).unwrap()
        };
        assert_eq!(read("DisplayName"), "Teleaf Notifications");
        assert_eq!(read("CustomActivator"), format!("{{{STUB_CLSID:?}}}"));
        // No toast is shown, no real app registration or account is touched.
        // Enabled/disabled are both valid; lookup itself must not throw 0x80070490.
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(&app_id))
            .expect("fresh registered AUMID must create a notifier");
        let setting = notifier
            .Setting()
            .expect("fresh registered AUMID must resolve its settings");
        println!(
            "Windows fresh notification identity: {}",
            setting_name(setting)
        );
        if std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
            && setting == NotificationSetting::Enabled
        {
            // Exercise Show on disposable CI desktops without a popup. Normal
            // local unit tests never request a notification or alter real app IDs.
            let document = XmlDocument::new().unwrap();
            document
                .LoadXml(&HSTRING::from(xml(
                    "Teleaf CI",
                    "Generated notification fixture",
                    true,
                )))
                .unwrap();
            let toast = ToastNotification::CreateToastNotification(&document).unwrap();
            let tag = HSTRING::from("fixture");
            let group = HSTRING::from("teleaf-ci");
            toast.SetTag(&tag).unwrap();
            toast.SetGroup(&group).unwrap();
            toast.SetSuppressPopup(true).unwrap();
            notifier
                .Show(&toast)
                .expect("fresh registered AUMID must accept a toast");
            missing_history_is_ok(
                ToastNotificationManager::History()
                    .unwrap()
                    .RemoveGroupedTagWithId(&tag, &group, &HSTRING::from(&app_id)),
            )
            .unwrap();
            println!("Windows notification Show / Remove API: PASS (CI fixture, popup suppressed)");
        }
    }

    #[test]
    fn missing_toast_removal_does_not_hide_other_windows_errors() {
        use windows::core::{Error, HRESULT};
        assert!(missing_history_is_ok(Ok(())).is_ok());
        assert!(
            missing_history_is_ok(Err(Error::from_hresult(HRESULT(0x80070490_u32 as i32)))).is_ok()
        );
        let denied = HRESULT(0x80070005_u32 as i32);
        assert_eq!(
            missing_history_is_ok(Err(Error::from_hresult(denied)))
                .unwrap_err()
                .code(),
            denied
        );
    }

    #[test]
    fn toast_text_is_xml_data_and_shortcut_has_our_identity() {
        let _apartment = Apartment::new().unwrap();
        let document = XmlDocument::new().unwrap();
        document
            .LoadXml(&HSTRING::from(xml(
                "群 <&>",
                "文字 </text><audio silent='false'/>",
                true,
            )))
            .unwrap();
        let nodes = document
            .GetElementsByTagName(&HSTRING::from("text"))
            .unwrap();
        assert_eq!(nodes.Length().unwrap(), 2);
        assert_eq!(
            nodes.Item(1).unwrap().InnerText().unwrap().to_string(),
            "文字 </text><audio silent='false'/>"
        );
        let folder =
            std::env::temp_dir().join(format!("teleaf-notification-test-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("test.lnk");
        shortcut(&path, &std::env::current_exe().unwrap(), APP_ID).unwrap();
        // SAFETY: COM is initialized and the test file path remains valid.
        unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            let file: IPersistFile = link.cast().unwrap();
            file.Load(
                PCWSTR(wide(&path).as_ptr()),
                windows::Win32::System::Com::STGM_READ,
            )
            .unwrap();
            let properties: IPropertyStore = link.cast().unwrap();
            let value = properties.GetValue(&APP_ID_KEY).unwrap();
            assert_eq!(value.Anonymous.Anonymous.vt, VT_LPWSTR);
            let identity = value
                .Anonymous
                .Anonymous
                .Anonymous
                .pwszVal
                .to_string()
                .unwrap();
            assert_eq!(identity, APP_ID);
            let value = properties.GetValue(&ACTIVATOR_KEY).unwrap();
            assert_eq!(
                value.Anonymous.Anonymous.vt,
                windows::Win32::System::Variant::VT_CLSID
            );
            assert_eq!(*value.Anonymous.Anonymous.Anonymous.puuid, STUB_CLSID);
        }
        std::fs::remove_dir_all(folder).unwrap();
    }
}
