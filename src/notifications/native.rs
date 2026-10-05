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
    InitPropVariantFromCLSID, PROPVARIANT, PropVariantClear,
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
    FOLDERID_Programs, IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath, ShellLink,
};
use windows::core::{GUID, HSTRING, Interface, PCWSTR, PWSTR};

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

fn shortcut(path: &Path, executable: &Path) -> windows::core::Result<()> {
    let executable = wide(executable);
    let path = wide(path);
    let app_id: Vec<u16> = APP_ID.encode_utf16().chain(Some(0)).collect();
    // SAFETY: COM is initialized; all strings remain valid through each copying call.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(PCWSTR(executable.as_ptr()))?;
        link.SetDescription(windows::core::w!("Teleaf Telegram terminal client"))?;
        let properties: IPropertyStore = link.cast()?;
        // A borrowed VT_LPWSTR; SetValue copies it. It must not be PropVariantClear'd.
        let mut value = PROPVARIANT::default();
        (*value.Anonymous.Anonymous).vt = VT_LPWSTR;
        (*value.Anonymous.Anonymous).Anonymous.pwszVal = PWSTR(app_id.as_ptr().cast_mut());
        properties.SetValue(&APP_ID_KEY, &value)?;
        // Microsoft-supported stub CLSID + protocol activation persists unpackaged toasts.
        let mut activator = InitPropVariantFromCLSID(&STUB_CLSID)?;
        let result = properties.SetValue(&ACTIVATOR_KEY, &activator);
        PropVariantClear(&mut activator)?;
        result?;
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
            .map_err(|e| e.to_string())?;
        let copied = folder.to_string();
        CoTaskMemFree(Some(folder.0.cast()));
        copied.map_err(|e| e.to_string())?
    };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
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
    shortcut(&folder.join("Teleaf Notifications.lnk"), &executable).map_err(|e| e.to_string())
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
            .map_err(|e| e.to_string())?;
        let result = RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&data)).ok();
        let _ = RegCloseKey(key);
        result.map_err(|e| e.to_string())
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
    let _apartment = Apartment::new().map_err(|e| e.to_string())?;
    register()?;
    let app_id = HSTRING::from(APP_ID);
    let account = HSTRING::from(account);
    let notifier =
        ToastNotificationManager::CreateToastNotifierWithId(&app_id).map_err(|e| e.to_string())?;
    if notifier.Setting().map_err(|e| e.to_string())? != NotificationSetting::Enabled {
        return Err("系统已禁用 Teleaf Notifications 的通知".into());
    }
    // An idle worker exits; the next notification creates one again, with no timer in the UI.
    while let Ok(command) = receiver.recv_timeout(Duration::from_secs(30)) {
        let result = match command {
            Command::Show {
                group,
                title,
                body,
                silent,
            } => (|| {
                let document = XmlDocument::new()?;
                document.LoadXml(&HSTRING::from(xml(&title, &body, silent)))?;
                let toast = ToastNotification::CreateToastNotification(&document)?;
                // One toast per Telegram group; new messages replace it in Action Center.
                toast.SetTag(&HSTRING::from(group.to_string()))?;
                toast.SetGroup(&account)?;
                notifier.Show(&toast)
            })(),
            Command::Remove(group) => ToastNotificationManager::History().and_then(|history| {
                history.RemoveGroupedTagWithId(&HSTRING::from(group.to_string()), &account, &app_id)
            }),
            Command::Clear => ToastNotificationManager::History()
                .and_then(|history| history.RemoveGroupWithId(&account, &app_id)),
        };
        result.map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        shortcut(&path, &std::env::current_exe().unwrap()).unwrap();
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
            let mut value = properties.GetValue(&APP_ID_KEY).unwrap();
            assert_eq!(value.Anonymous.Anonymous.vt, VT_LPWSTR);
            let identity = value
                .Anonymous
                .Anonymous
                .Anonymous
                .pwszVal
                .to_string()
                .unwrap();
            windows::Win32::System::Com::StructuredStorage::PropVariantClear(&mut value).unwrap();
            assert_eq!(identity, APP_ID);
            let mut value = properties.GetValue(&ACTIVATOR_KEY).unwrap();
            assert_eq!(
                value.Anonymous.Anonymous.vt,
                windows::Win32::System::Variant::VT_CLSID
            );
            assert_eq!(*value.Anonymous.Anonymous.Anonymous.puuid, STUB_CLSID);
            PropVariantClear(&mut value).unwrap();
        }
        std::fs::remove_dir_all(folder).unwrap();
    }
}
