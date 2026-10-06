//! Read the legacy Windows screenshot format on the existing clipboard worker.
use windows_sys::Win32::{
    Foundation::HGLOBAL,
    System::{
        DataExchange::{
            CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
        },
        Memory::{GlobalLock, GlobalSize, GlobalUnlock},
    },
};

struct Clipboard;
impl Drop for Clipboard {
    fn drop(&mut self) {
        // SAFETY: this guard is created only after opening on the current thread.
        unsafe { CloseClipboard() };
    }
}
struct Locked(HGLOBAL);
impl Drop for Locked {
    fn drop(&mut self) {
        // SAFETY: this borrowed clipboard object was successfully locked and
        // must be unlocked, never freed by the reader.
        unsafe { GlobalUnlock(self.0) };
    }
}

pub(super) fn read_dib() -> Result<Option<image::DynamicImage>, String> {
    // Clipboard contention is transient. Bound retries and keep them off the UI.
    let mut clipboard = None;
    for attempt in 0..5 {
        // SAFETY: a reader needs no owner window and does not mutate clipboard data.
        if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
            clipboard = Some(Clipboard);
            break;
        }
        if attempt < 4 {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    let _clipboard = clipboard.ok_or("系统剪贴板正在被占用，请稍后再按 F7")?;
    const CF_DIB: u32 = 8;
    // SAFETY: the clipboard remains open throughout the handle borrow and decode.
    unsafe {
        if IsClipboardFormatAvailable(CF_DIB) == 0 {
            return Ok(None);
        }
        let handle = GetClipboardData(CF_DIB);
        if handle.is_null() {
            return Err("无法读取剪贴板 CF_DIB 位图，请重新复制图片后按 F7".into());
        }
        let size = GlobalSize(handle);
        if size == 0 {
            return Err("剪贴板 CF_DIB 位图为空，请重新复制图片".into());
        }
        if size > 64 * 1024 * 1024 {
            return Err("剪贴板位图超过 64 MiB；请以原文件发送".into());
        }
        let bytes = GlobalLock(handle);
        if bytes.is_null() {
            return Err("无法锁定剪贴板 CF_DIB 位图，请重新复制图片后按 F7".into());
        }
        let _locked = Locked(handle);
        // The slice is only used while both RAII guards are alive; the bounded
        // decoder returns owned pixels. PNG encoding happens after unlocking.
        super::dib::decode(std::slice::from_raw_parts(bytes.cast(), size)).map(Some)
    }
}
