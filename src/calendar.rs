//! Local message timestamps without a clock tick or background timer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
}
impl Stamp {
    pub fn date(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
    pub fn time(self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }
    pub fn same_date(self, other: Self) -> bool {
        (self.year, self.month, self.day) == (other.year, other.month, other.day)
    }
}

#[cfg(unix)]
pub fn local(timestamp: i64) -> Option<Stamp> {
    if timestamp <= 0 {
        return None;
    }
    let seconds = timestamp as libc::time_t;
    let mut date = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: localtime_r writes to a valid caller-owned tm; it has no shared
    // output buffer. The application does not mutate TZ or the C environment.
    let result = unsafe { libc::localtime_r(&seconds, date.as_mut_ptr()) };
    if result.is_null() {
        return None;
    }
    // SAFETY: a non-null return confirms date was initialized.
    let date = unsafe { date.assume_init() };
    Some(Stamp {
        year: date.tm_year + 1900,
        month: (date.tm_mon + 1) as u8,
        day: date.tm_mday as u8,
        hour: date.tm_hour as u8,
        minute: date.tm_min as u8,
    })
}

#[cfg(windows)]
pub fn local(timestamp: i64) -> Option<Stamp> {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    if timestamp <= 0 {
        return None;
    }
    let ticks = (timestamp as u64)
        .checked_add(11_644_473_600)?
        .checked_mul(10_000_000)?;
    let file = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut date = SYSTEMTIME::default();
    // SAFETY: all pointers refer to initialized, correctly sized structs; null
    // timezone selects the system's timezone and daylight rules.
    if unsafe { FileTimeToSystemTime(&file, &mut utc) } == 0
        || unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut date) } == 0
    {
        return None;
    }
    Some(Stamp {
        year: i32::from(date.wYear),
        month: date.wMonth as u8,
        day: date.wDay as u8,
        hour: date.wHour as u8,
        minute: date.wMinute as u8,
    })
}

#[cfg(not(any(unix, windows)))]
pub fn local(_: i64) -> Option<Stamp> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_dates_do_not_become_epoch_labels_and_local_stamp_is_stable() {
        assert!(local(0).is_none());
        let date = local(1_700_000_000).unwrap();
        assert!((1..=12).contains(&date.month) && (1..=31).contains(&date.day));
        assert!(date.hour < 24 && date.minute < 60);
        assert_eq!(date, local(1_700_000_000).unwrap());
        assert!(date.same_date(date));
    }
}
