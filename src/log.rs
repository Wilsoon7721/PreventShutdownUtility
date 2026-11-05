//! Opt-in diagnostic log, written only when `--log` is passed

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

const FILE_NAME: &str = "PreventShutdownApp.log";

static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Turns logging on. Returns the file actually being written if any
pub fn init(explicit: Option<&str>) -> Option<PathBuf> {
    let chosen = pick(explicit);
    PATH.set(chosen.clone()).ok();
    chosen
}

fn pick(explicit: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = explicit.filter(|p| !p.is_empty()) {
        let path = PathBuf::from(p);
        let path = if path.is_dir() {
            path.join(FILE_NAME)
        } else {
            path
        };
        return writable(&path).then_some(path);
    }

    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(FILE_NAME)));
    if let Some(path) = beside_exe {
        if writable(&path) {
            return Some(path);
        }
    }

    let temp = std::env::temp_dir().join(FILE_NAME);
    writable(&temp).then_some(temp)
}

fn writable(path: &Path) -> bool {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .is_ok()
}

pub fn enabled() -> bool {
    matches!(PATH.get(), Some(Some(_)))
}

/// Appends one timestamped line
pub fn line(args: std::fmt::Arguments) {
    let Some(Some(path)) = PATH.get() else {
        return;
    };
    let stamp = unsafe {
        let mut t: SYSTEMTIME = std::mem::zeroed();
        GetLocalTime(&mut t);
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
        )
    };
    let _guard = WRITE_LOCK.lock();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{stamp}  {args}");
        let _ = f.flush();
    }
}

#[macro_export]
macro_rules! log_line {
    ($($arg:tt)*) => {
        if $crate::log::enabled() {
            $crate::log::line(format_args!($($arg)*));
        }
    };
}

/// Decodes the lParam flags of WM_QUERYENDSESSION / WM_ENDSESSION
pub fn endsession_flags(lparam: isize) -> String {
    const FLAGS: [(isize, &str); 3] = [
        (0x0000_0001, "ENDSESSION_CLOSEAPP"),
        (0x4000_0000, "ENDSESSION_CRITICAL"),
        (0x8000_0000, "ENDSESSION_LOGOFF"),
    ];
    let mut named: Vec<&str> = FLAGS
        .iter()
        .filter(|(bit, _)| lparam & bit != 0)
        .map(|(_, name)| *name)
        .collect();
    if named.is_empty() {
        named.push("none (plain shutdown/restart)");
    }
    named.join(" | ")
}
