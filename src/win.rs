use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

use windows_sys::Win32::Foundation::{
    CloseHandle, BOOL, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT, POINT, WPARAM,
};
use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows_sys::Win32::Storage::FileSystem::{
    GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};
use windows_sys::Win32::System::Console::{
    AttachConsole, FreeConsole, GetConsoleTitleW, SetConsoleCtrlHandler,
};
use windows_sys::Win32::System::Diagnostics::Debug::{SetErrorMode, SEM_FAILCRITICALERRORS};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Power::{
    GetSystemPowerStatus, PlatformRoleMobile, PlatformRoleSlate, PowerDeterminePlatformRoleEx,
    POWER_PLATFORM_ROLE_V2, SYSTEM_POWER_STATUS,
};
use windows_sys::Win32::System::RemoteDesktop::{
    ProcessIdToSessionId, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
    NOTIFY_FOR_THIS_SESSION,
};
use windows_sys::Win32::System::Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy};
use windows_sys::Win32::System::SystemInformation::{GetSystemFirmwareTable, RSMB};
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, SetProcessShutdownParameters};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, EnumWindows, FindWindowExW, GetCursorPos, GetMessageW, GetWindow,
    GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, KillTimer, LoadIconW, MessageBoxW,
    PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetForegroundWindow,
    SetTimer, TrackPopupMenu, TranslateMessage, CW_USEDEFAULT, GW_OWNER, IDI_INFORMATION,
    IDI_WARNING, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MF_CHECKED, MF_GRAYED,
    MF_SEPARATOR, MF_STRING, MSG, TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP,
    WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_DEVICECHANGE, WM_ENDSESSION, WM_LBUTTONUP, WM_NULL,
    WM_POWERBROADCAST, WM_QUERYENDSESSION, WM_RBUTTONUP, WM_TIMER, WM_WTSSESSION_CHANGE, WNDCLASSW,
    WS_OVERLAPPED,
};

use crate::config::{compose_reason, Condition, Config, LaunchInfo};
use crate::machine::{parse_smbios_chassis, MachineInfo};

const CLASS_NAME: &str = "PreventShutdownApp.Blocker";
const JOB_CLASS_NAME: &str = "PreventShutdownApp.Job";
const POLL_TIMER_ID: usize = 2;
const POLL_INTERVAL_MS: u32 = 2000;
const SYNC_TIMER_ID: usize = 3;
const SYNC_DELAY_MS: u32 = 300;
const DRIVE_REMOVABLE: u32 = 2;
const ENDSESSION_CRITICAL: isize = 0x4000_0000;

static CONFIG: OnceLock<Config> = OnceLock::new();
static CURRENT_REASON: Mutex<String> = Mutex::new(String::new());
static PLUGGED_DRIVES: Mutex<Vec<char>> = Mutex::new(Vec::new());
static JOB_PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());
static WATCHED_TITLE: Mutex<Option<String>> = Mutex::new(None);
static JOB_SEEN: AtomicBool = AtomicBool::new(false);
static JOB_DONE: AtomicBool = AtomicBool::new(false);
static CHILD: Mutex<Option<Child>> = Mutex::new(None);
static EXIT_CODE: AtomicI32 = AtomicI32::new(0);
static START_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
static OWN_SESSION: AtomicU32 = AtomicU32::new(u32::MAX);
static SYNCING: AtomicBool = AtomicBool::new(false);
static RESYNC: AtomicBool = AtomicBool::new(false);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
static BLOCKING: AtomicBool = AtomicBool::new(false);
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

const WM_TRAY: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;
const CMD_QUIT: usize = 1;
const CMD_LOCK_REMINDER: usize = 2;
const CMD_TEST_REMINDER: usize = 3;

const WTS_SESSION_LOCK: usize = 7;
const BEEP_TIMER_ID: usize = 1;
const BEEP_INTERVAL_MS: u32 = 700;
const BEEP_COUNT: u32 = 2;

static LOCK_REMINDER: AtomicBool = AtomicBool::new(false);
static REMINDER_EXPLAINED: AtomicBool = AtomicBool::new(false);
static BEEPS_LEFT: AtomicU32 = AtomicU32::new(0);
static LOCK_EVENTS_OK: AtomicBool = AtomicBool::new(false);
static BEEP: OnceLock<Vec<u8>> = OnceLock::new();

fn play_beep() {
    let wav = BEEP.get_or_init(crate::sound::beep_wav);
    unsafe {
        PlaySoundW(
            wav.as_ptr().cast(),
            std::ptr::null_mut(),
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn message_box(title: &str, text: &str, error: bool) {
    let icon = if error {
        MB_ICONERROR
    } else {
        MB_ICONINFORMATION
    };
    let (t, b) = (wide(title), wide(text));
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            b.as_ptr(),
            t.as_ptr(),
            MB_OK | MB_SETFOREGROUND | icon,
        )
    };
}

pub fn prepare_process() -> LaunchInfo {
    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS);
        SetProcessShutdownParameters(0x3FF, 0);
    }

    let removable_exe_drive = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().and_then(|s| s.chars().next()))
        .map(|c| c.to_ascii_uppercase())
        .filter(|c| c.is_ascii_uppercase())
        .filter(|&c| is_removable(c));

    if let Ok(dir) = std::env::current_dir() {
        START_DIR.set(dir).ok();
    }
    let safe_dir = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let _ = std::env::set_current_dir(safe_dir);

    LaunchInfo {
        removable_exe_drive,
        machine: machine_info(),
    }
}

fn machine_info() -> MachineInfo {
    let chassis = unsafe {
        let size = GetSystemFirmwareTable(RSMB, 0, std::ptr::null_mut(), 0);
        let mut buf = vec![0u8; size as usize];
        if size > 0 && GetSystemFirmwareTable(RSMB, 0, buf.as_mut_ptr(), size) == size {
            parse_smbios_chassis(&buf)
        } else {
            None
        }
    };
    let role = unsafe { PowerDeterminePlatformRoleEx(POWER_PLATFORM_ROLE_V2) };
    let has_battery = power_status().is_some_and(|p| p.BatteryFlag != 128 && p.BatteryFlag != 255);
    MachineInfo {
        chassis,
        mobile_platform_role: Some(role == PlatformRoleMobile || role == PlatformRoleSlate),
        has_battery,
    }
}

fn power_status() -> Option<SYSTEM_POWER_STATUS> {
    unsafe {
        let mut p: SYSTEM_POWER_STATUS = std::mem::zeroed();
        (GetSystemPowerStatus(&mut p) != 0).then_some(p)
    }
}

fn on_ac_power() -> bool {
    power_status().is_some_and(|p| p.ACLineStatus == 1)
}

fn is_removable(letter: char) -> bool {
    let root = wide(&format!("{letter}:\\"));
    unsafe { GetDriveTypeW(root.as_ptr()) == DRIVE_REMOVABLE }
}

fn removable_drives() -> Vec<char> {
    let mask = unsafe { GetLogicalDrives() };
    ('A'..='Z')
        .enumerate()
        .filter(|(i, _)| mask & (1 << i) != 0)
        .map(|(_, c)| c)
        .filter(|&c| is_removable(c) && drive_present(c))
        .collect()
}

fn session_of(pid: u32) -> Option<u32> {
    let mut session = 0;
    (unsafe { ProcessIdToSessionId(pid, &mut session) } != 0).then_some(session)
}

fn find_processes(exe: &str) -> Vec<u32> {
    let own = OWN_SESSION.load(Ordering::Relaxed);
    let mut pids = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return pids;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut entry) != 0;
        while ok {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
            let pid = entry.th32ProcessID;
            if name.eq_ignore_ascii_case(exe) && session_of(pid) == Some(own) {
                pids.push(pid);
            }
            ok = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
    }
    pids
}

struct TitleSearch<'a> {
    pids: &'a [u32],
    found: Option<String>,
}

unsafe extern "system" fn title_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let search = &mut *(lparam as *mut TitleSearch);
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if !search.pids.contains(&pid)
        || IsWindowVisible(hwnd) == 0
        || !GetWindow(hwnd, GW_OWNER).is_null()
    {
        return 1;
    }
    let mut buf = [0u16; 512];
    let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if n > 0 {
        search.found = Some(String::from_utf16_lossy(&buf[..n as usize]));
        return 0;
    }
    1
}

fn console_title(pid: u32) -> Option<String> {
    unsafe {
        if AttachConsole(pid) == 0 {
            return None;
        }
        let mut buf = [0u16; 512];
        let n = GetConsoleTitleW(buf.as_mut_ptr(), buf.len() as u32) as usize;
        FreeConsole();
        let title = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
        let title = title.trim();
        (!title.is_empty() && !title.to_ascii_lowercase().ends_with(".exe"))
            .then(|| title.to_string())
    }
}

fn watched_window_title(pids: &[u32]) -> Option<String> {
    if pids.is_empty() {
        return None;
    }
    let mut search = TitleSearch { pids, found: None };
    unsafe {
        EnumWindows(
            Some(title_callback),
            &mut search as *mut TitleSearch as LPARAM,
        )
    };
    search
        .found
        .or_else(|| pids.iter().find_map(|&pid| console_title(pid)))
}

fn child_running() -> bool {
    let mut guard = lock(&CHILD);
    let Some(child) = guard.as_mut() else {
        return false;
    };
    match child.try_wait() {
        Ok(None) => {
            *lock(&JOB_PIDS) = vec![child.id()];
            true
        }
        Ok(Some(status)) => {
            EXIT_CODE.store(status.code().unwrap_or(1), Ordering::Relaxed);
            JOB_DONE.store(true, Ordering::Relaxed);
            lock(&JOB_PIDS).clear();
            false
        }
        Err(_) => {
            EXIT_CODE.store(1, Ordering::Relaxed);
            JOB_DONE.store(true, Ordering::Relaxed);
            false
        }
    }
}

fn drive_list(drives: &[char]) -> String {
    drives
        .iter()
        .map(char::to_string)
        .collect::<Vec<_>>()
        .join(":, ")
}

fn drive_present(letter: char) -> bool {
    let bit = (letter as u32) - ('A' as u32);
    if unsafe { GetLogicalDrives() } & (1 << bit) == 0 {
        return false;
    }
    let root = wide(&format!("{letter}:\\"));
    unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        ) != 0
    }
}

fn should_block() -> bool {
    match CONFIG.get().map(|c| &c.condition) {
        Some(Condition::Always) => true,
        Some(Condition::DrivePresent(d)) => drive_present(*d),
        Some(Condition::AnyRemovableDrive) => {
            let drives = removable_drives();
            let block = !drives.is_empty();
            *lock(&PLUGGED_DRIVES) = drives;
            block
        }
        Some(Condition::OnAcPower) => on_ac_power(),
        Some(Condition::ProcessRunning(exe)) => {
            let pids = find_processes(exe);
            let running = !pids.is_empty();
            if running {
                JOB_SEEN.store(true, Ordering::Relaxed);
            } else if JOB_SEEN.load(Ordering::Relaxed) {
                JOB_DONE.store(true, Ordering::Relaxed);
            }
            *lock(&JOB_PIDS) = pids;
            running
        }
        Some(Condition::Command(_)) => child_running(),
        None => false,
    }
}

fn sync_block_reason(hwnd: HWND) -> bool {
    if SYNCING.swap(true, Ordering::Acquire) {
        RESYNC.store(true, Ordering::Relaxed);
        return BLOCKING.load(Ordering::Relaxed);
    }
    let mut block = BLOCKING.load(Ordering::Relaxed);
    for _ in 0..5 {
        RESYNC.store(false, Ordering::Relaxed);
        block = sync_once(hwnd);
        if !RESYNC.load(Ordering::Relaxed) {
            break;
        }
    }
    SYNCING.store(false, Ordering::Release);
    block
}

fn sync_once(hwnd: HWND) -> bool {
    let Some(config) = CONFIG.get() else {
        return false;
    };
    let block = should_block();
    let reason = match config.condition {
        Condition::AnyRemovableDrive => {
            let drives = lock(&PLUGGED_DRIVES).clone();
            config.reason.replace("{drive}", &drive_list(&drives))
        }
        _ if config.show_window_title && block => {
            let pids = lock(&JOB_PIDS).clone();
            let title = watched_window_title(&pids);
            let reason = compose_reason(&config.reason, title.as_deref());
            *lock(&WATCHED_TITLE) = title;
            reason
        }
        _ => config.reason.clone(),
    };
    let was_blocking = BLOCKING.load(Ordering::Relaxed);
    let reason_changed = *lock(&CURRENT_REASON) != reason;
    if block != was_blocking || (block && reason_changed) {
        unsafe {
            if was_blocking {
                ShutdownBlockReasonDestroy(hwnd);
            }
            if block {
                ShutdownBlockReasonCreate(hwnd, wide(&reason).as_ptr());
            }
        }
        *lock(&CURRENT_REASON) = reason;
        BLOCKING.store(block, Ordering::Relaxed);
        tray(hwnd, NIM_MODIFY);
    }
    block
}

fn status_text() -> String {
    let base = base_status_text();
    match lock(&WATCHED_TITLE).as_deref() {
        Some(t) if BLOCKING.load(Ordering::Relaxed) => format!("{base}\nNow: {t}"),
        _ => base,
    }
}

fn base_status_text() -> String {
    let blocking = BLOCKING.load(Ordering::Relaxed);
    match CONFIG.get().map(|c| &c.condition) {
        Some(Condition::DrivePresent(d)) if blocking => {
            format!("Blocking shutdown: {d}: is plugged in")
        }
        Some(Condition::DrivePresent(d)) => format!("Not blocking: {d}: is unplugged"),
        Some(Condition::AnyRemovableDrive) if blocking => {
            let drives = lock(&PLUGGED_DRIVES);
            let verb = if drives.len() == 1 { "is" } else { "are" };
            format!(
                "Blocking shutdown: {}: {verb} plugged in",
                drive_list(&drives)
            )
        }
        Some(Condition::AnyRemovableDrive) => "Not blocking: no USB drive plugged in".into(),
        Some(Condition::OnAcPower) if blocking => "Blocking shutdown: charger is plugged in".into(),
        Some(Condition::OnAcPower) => "Not blocking: running on battery".into(),
        Some(Condition::ProcessRunning(exe)) if blocking => {
            format!("Blocking shutdown: {exe} is running")
        }
        Some(Condition::ProcessRunning(exe)) => format!("Waiting for {exe} to start"),
        Some(Condition::Command(_)) if blocking => {
            "Blocking shutdown until the command finishes".into()
        }
        Some(Condition::Command(_)) => "Command finished".into(),
        _ => "Blocking shutdown".to_string(),
    }
}

fn tray(hwnd: HWND, op: u32) {
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = TRAY_ID;
        if op != NIM_DELETE {
            nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
            nid.uCallbackMessage = WM_TRAY;
            let icon = if BLOCKING.load(Ordering::Relaxed) {
                IDI_WARNING
            } else {
                IDI_INFORMATION
            };
            nid.hIcon = LoadIconW(std::ptr::null_mut(), icon);
            let title = match CONFIG.get() {
                Some(c) if BLOCKING.load(Ordering::Relaxed) => c.title.as_str(),
                _ => "PreventShutdownApp",
            };
            let tip: Vec<u16> = format!("{title}\n{}", status_text())
                .encode_utf16()
                .collect();
            copy_truncated(&mut nid.szTip, &tip);
        }
        Shell_NotifyIconW(op, &nid);
    }
}

fn copy_truncated(dst: &mut [u16], src: &[u16]) {
    let n = src.len().min(dst.len() - 1);
    dst[..n].copy_from_slice(&src[..n]);
}

fn lock_reminder(hwnd: HWND) {
    play_beep();
    BEEPS_LEFT.store(BEEP_COUNT - 1, Ordering::Relaxed);
    unsafe { SetTimer(hwnd, BEEP_TIMER_ID, BEEP_INTERVAL_MS, None) };
}

fn explain_lock_reminder() {
    let Some(config) = CONFIG.get() else { return };
    message_box(
        "Lock reminder",
        &format!("Lock reminder is on.\n\n{}\n\nWindows doesn't let apps show messages on the lock screen, so the beeps are the only reminder. Use \"Test lock reminder\" in this menu to hear them.", config.reminder.explanation),
        false,
    );
}

fn show_tray_menu(hwnd: HWND) {
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        let mut status_line = status_text().replace('\n', " — ");
        if status_line.chars().count() > 90 {
            status_line = status_line.chars().take(89).collect::<String>() + "…";
        }
        let status = wide(&status_line);
        let quit = wide("Quit PreventShutdownApp");
        let reminder = wide(
            CONFIG
                .get()
                .map(|c| c.reminder.menu_label.as_str())
                .unwrap_or("Remind me when computer locks"),
        );
        let test_sound = wide("Test lock reminder");
        let no_lock_events = wide("(Windows isn't reporting lock events to this app)");
        let reminder_flags = if LOCK_REMINDER.load(Ordering::Relaxed) {
            MF_CHECKED
        } else {
            0
        };
        AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, status.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(
            menu,
            MF_STRING | reminder_flags,
            CMD_LOCK_REMINDER,
            reminder.as_ptr(),
        );
        AppendMenuW(menu, MF_STRING, CMD_TEST_REMINDER, test_sound.as_ptr());
        if !LOCK_EVENTS_OK.load(Ordering::Relaxed) {
            AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, no_lock_events.as_ptr());
        }
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, CMD_QUIT, quit.as_ptr());

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            pt.x,
            pt.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);

        match cmd as usize {
            CMD_QUIT => {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
            CMD_TEST_REMINDER => lock_reminder(hwnd),
            CMD_LOCK_REMINDER => {
                let was_on = LOCK_REMINDER.fetch_xor(true, Ordering::Relaxed);
                if !was_on && !REMINDER_EXPLAINED.swap(true, Ordering::Relaxed) {
                    explain_lock_reminder();
                }
            }
            _ => {}
        }
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_QUERYENDSESSION => {
            if lparam & ENDSESSION_CRITICAL != 0 {
                return 1;
            }
            if sync_block_reason(hwnd) {
                0
            } else {
                1
            }
        }
        WM_ENDSESSION => 0,
        WM_DEVICECHANGE | WM_POWERBROADCAST => {
            SetTimer(hwnd, SYNC_TIMER_ID, SYNC_DELAY_MS, None);
            1
        }
        WM_TIMER if wparam == SYNC_TIMER_ID => {
            KillTimer(hwnd, SYNC_TIMER_ID);
            sync_block_reason(hwnd);
            0
        }
        WM_TRAY => {
            match lparam as u32 {
                WM_RBUTTONUP | WM_LBUTTONUP | WM_CONTEXTMENU => show_tray_menu(hwnd),
                _ => {}
            }
            0
        }
        m if Some(&m) == TASKBAR_CREATED.get() => {
            tray(hwnd, NIM_ADD);
            0
        }
        WM_WTSSESSION_CHANGE => {
            if wparam == WTS_SESSION_LOCK
                && LOCK_REMINDER.load(Ordering::Relaxed)
                && sync_block_reason(hwnd)
            {
                lock_reminder(hwnd);
            }
            0
        }
        WM_TIMER if wparam == POLL_TIMER_ID => {
            sync_block_reason(hwnd);
            if JOB_DONE.load(Ordering::Relaxed) {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_TIMER if wparam == BEEP_TIMER_ID => {
            let left = BEEPS_LEFT.load(Ordering::Relaxed);
            if left == 0 || !should_block() {
                KillTimer(hwnd, BEEP_TIMER_ID);
            } else {
                play_beep();
                BEEPS_LEFT.store(left - 1, Ordering::Relaxed);
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            tray(hwnd, NIM_DELETE);
            WTSUnRegisterSessionNotification(hwnd);
            if BLOCKING.swap(false, Ordering::Relaxed) {
                ShutdownBlockReasonDestroy(hwnd);
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn run(config: Config) -> Result<i32, String> {
    let title_w = wide(&config.title);
    let is_job = config.condition.is_job();
    OWN_SESSION.store(
        session_of(unsafe { GetCurrentProcessId() }).unwrap_or(u32::MAX),
        Ordering::Relaxed,
    );
    if let Condition::Command(cmd) = &config.condition {
        let mut command = std::process::Command::new(&cmd[0]);
        command.args(&cmd[1..]);
        if let Some(dir) = START_DIR.get() {
            command.current_dir(dir);
        }
        let child = command
            .spawn()
            .map_err(|e| format!("Couldn't start \"{}\": {e}", cmd[0]))?;
        *lock(&CHILD) = Some(child);
        unsafe { SetConsoleCtrlHandler(None, 1) };
    }
    LOCK_REMINDER.store(config.lock_reminder, Ordering::Relaxed);
    REMINDER_EXPLAINED.store(config.lock_reminder, Ordering::Relaxed);
    CONFIG.set(config).ok();

    let class_w = wide(if is_job { JOB_CLASS_NAME } else { CLASS_NAME });
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
            lpszClassName: class_w.as_ptr(),
            ..std::mem::zeroed()
        };
        if RegisterClassW(&wc) == 0 {
            return Err("RegisterClassW failed".into());
        }

        let hwnd = CreateWindowExW(
            0,
            class_w.as_ptr(),
            title_w.as_ptr(),
            WS_OVERLAPPED,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return Err("CreateWindowExW failed".into());
        }

        let taskbar_created = wide("TaskbarCreated");
        TASKBAR_CREATED
            .set(RegisterWindowMessageW(taskbar_created.as_ptr()))
            .ok();

        let ok = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) != 0;
        LOCK_EVENTS_OK.store(ok, Ordering::Relaxed);

        sync_block_reason(hwnd);
        tray(hwnd, NIM_ADD);
        if is_job {
            SetTimer(hwnd, POLL_TIMER_ID, POLL_INTERVAL_MS, None);
        }

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    if lock(&CHILD).is_some() && !JOB_DONE.load(Ordering::Relaxed) {
        return Ok(1);
    }
    Ok(EXIT_CODE.load(Ordering::Relaxed))
}

pub fn stop_all() -> usize {
    let class_w = wide(CLASS_NAME);
    let mut count = 0;
    let mut after: HWND = std::ptr::null_mut();
    loop {
        let hwnd = unsafe {
            FindWindowExW(
                std::ptr::null_mut(),
                after,
                class_w.as_ptr(),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            break;
        }
        unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
        count += 1;
        after = hwnd;
    }
    count
}
