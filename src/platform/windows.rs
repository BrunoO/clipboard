#![allow(unsafe_code)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use crate::logging::log_message;

#[derive(Clone, Copy)]
struct CapturedWindow {
    hwnd: isize,
    pid: u32,
}

static PREVIOUS_WINDOW: Mutex<Option<CapturedWindow>> = Mutex::new(None);

fn load_previous_window() -> Option<CapturedWindow> {
    *PREVIOUS_WINDOW
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn store_previous_window(hwnd: isize, pid: u32) {
    *PREVIOUS_WINDOW
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(CapturedWindow { hwnd, pid });
}

fn clear_previous_window_if_matches(stale_hwnd: isize) {
    let mut guard = PREVIOUS_WINDOW
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if guard.is_some_and(|w| w.hwnd == stale_hwnd) {
        *guard = None;
    }
}

type Hwnd = *mut std::ffi::c_void;

// Calling PeekMessage on a thread creates its message queue. This is a
// prerequisite for AttachThreadInput, which a bare std::thread lacks.
#[allow(clippy::upper_case_acronyms)]
#[repr(C)]
struct MSG {
    hwnd: Hwnd,
    message: u32,
    w_param: usize,
    l_param: isize,
    time: u32,
    pt_x: i32,
    pt_y: i32,
    l_private: u32,
}

const PM_NOREMOVE: u32 = 0;
const SW_SHOW: i32 = 5;
const SW_RESTORE: i32 = 9;
const MAPVK_VK_TO_VSC: u32 = 0;
const MAPVK_VSC_TO_VK: u32 = 1;

extern "system" {
    fn GetForegroundWindow() -> Hwnd;
    fn SetForegroundWindow(hwnd: Hwnd) -> i32;
    fn BringWindowToTop(hwnd: Hwnd) -> i32;
    fn ShowWindow(hwnd: Hwnd, nCmdShow: i32) -> i32;
    fn IsIconic(hwnd: Hwnd) -> i32;
    fn IsWindow(hwnd: Hwnd) -> i32;
    fn PeekMessageW(
        lpMsg: *mut MSG,
        hWnd: Hwnd,
        wMsgFilterMin: u32,
        wMsgFilterMax: u32,
        wRemoveMsg: u32,
    ) -> i32;
    fn GetCurrentProcessId() -> u32;
    fn GetCurrentThreadId() -> u32;
    fn GetWindowThreadProcessId(hwnd: Hwnd, lpdwProcessId: *mut u32) -> u32;
    fn AttachThreadInput(idAttach: u32, idAttachTo: u32, fAttach: i32) -> i32;
    fn EnumWindows(lpEnumFunc: unsafe extern "system" fn(Hwnd, isize) -> i32, lParam: isize)
        -> i32;
    fn GetWindowTextW(hwnd: Hwnd, lpString: *mut u16, nMaxCount: i32) -> i32;
    fn GetKeyboardLayout(idThread: u32) -> *mut std::ffi::c_void;
    fn MapVirtualKeyExW(uCode: u32, uMapType: u32, dwhkl: *mut std::ffi::c_void) -> u32;
    fn ToUnicodeEx(
        wVirtKey: u32,
        wScanCode: u32,
        lpKeyState: *const u8,
        pwszBuff: *mut u16,
        cchBuff: i32,
        wFlags: u32,
        dwhkl: *mut std::ffi::c_void,
    ) -> i32;
}

/// Resolves the keyboard layout (HKL) for the thread whose layout the user is
/// currently typing with, preferring the foreground window's thread (or the
/// captured target window when our own window is foreground) over our UI thread.
unsafe fn target_keyboard_layout() -> *mut std::ffi::c_void {
    let current_pid = GetCurrentProcessId();
    let fg = GetForegroundWindow();
    let mut fg_pid = 0u32;
    let fg_thread = if fg.is_null() {
        0
    } else {
        GetWindowThreadProcessId(fg, &raw mut fg_pid)
    };

    let target_thread = if fg_thread != 0 && fg_pid != current_pid {
        fg_thread
    } else if let Some(prev) = load_previous_window() {
        let hwnd = prev.hwnd as Hwnd;
        if IsWindow(hwnd) != 0 {
            GetWindowThreadProcessId(hwnd, std::ptr::null_mut())
        } else {
            0
        }
    } else {
        0
    };

    let hkl = if target_thread != 0 {
        GetKeyboardLayout(target_thread)
    } else {
        std::ptr::null_mut()
    };
    if hkl.is_null() {
        GetKeyboardLayout(0)
    } else {
        hkl
    }
}

fn find_main_window() -> Option<Hwnd> {
    unsafe extern "system" fn enum_proc(hwnd: Hwnd, lparam: isize) -> i32 {
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &raw mut pid);
        if pid == GetCurrentProcessId() {
            let mut buf = [0u16; 64];
            let len = GetWindowTextW(
                hwnd,
                buf.as_mut_ptr(),
                i32::try_from(buf.len()).unwrap_or(0),
            );
            if let Ok(len_usize) = usize::try_from(len) {
                if len_usize > 0 {
                    let title = String::from_utf16_lossy(&buf[..len_usize]);
                    if title == "CipherClipboard" {
                        let out = lparam as *mut Hwnd;
                        if !out.is_null() {
                            *out = hwnd;
                        }
                        return 0; // stop enumeration
                    }
                }
            }
        }
        1
    }

    let mut target_hwnd: Hwnd = std::ptr::null_mut();
    unsafe {
        EnumWindows(enum_proc, std::ptr::addr_of_mut!(target_hwnd) as isize);
    }
    if target_hwnd.is_null() {
        None
    } else {
        Some(target_hwnd)
    }
}

pub(super) fn order_front_main_window() {
    let Some(target_hwnd) = find_main_window() else {
        return;
    };

    unsafe {
        if IsIconic(target_hwnd) != 0 {
            ShowWindow(target_hwnd, SW_RESTORE);
        } else {
            ShowWindow(target_hwnd, SW_SHOW);
        }

        let fg_hwnd = GetForegroundWindow();
        let cur_thread = GetCurrentThreadId();
        let fg_thread = if fg_hwnd.is_null() {
            0
        } else {
            let mut fg_pid: u32 = 0;
            GetWindowThreadProcessId(fg_hwnd, &raw mut fg_pid)
        };

        let attached = fg_thread != 0
            && fg_thread != cur_thread
            && AttachThreadInput(cur_thread, fg_thread, 1) != 0;

        BringWindowToTop(target_hwnd);
        SetForegroundWindow(target_hwnd);

        if attached {
            let _ = AttachThreadInput(cur_thread, fg_thread, 0);
        }
    }
}

pub(super) fn is_active() -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return false;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &raw mut pid);
        pid == GetCurrentProcessId()
    }
}

pub(super) fn capture_frontmost() {
    unsafe {
        let hwnd = GetForegroundWindow();
        if !hwnd.is_null() {
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &raw mut pid);
            if pid != 0 && pid != GetCurrentProcessId() {
                store_previous_window(hwnd as isize, pid);
            }
        }
    }
}

pub(super) fn restore_focus() {
    let Some(prev) = load_previous_window() else {
        log_message(
            "WARN",
            "Focus restore skipped: no previously captured window",
        );
        return;
    };
    let hwnd_raw = prev.hwnd;

    unsafe {
        let target_hwnd = hwnd_raw as Hwnd;
        if IsWindow(target_hwnd) == 0 {
            clear_previous_window_if_matches(hwnd_raw);
            log_message(
                "WARN",
                "Focus restore skipped: captured target window no longer exists",
            );
            return;
        }

        // A bare worker thread has no message queue and AttachThreadInput fails
        // unless both threads have one. PeekMessage creates it as a side effect.
        let mut msg = std::mem::zeroed::<MSG>();
        let _ = PeekMessageW(&raw mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);

        let cur_thread = GetCurrentThreadId();
        let fg_hwnd = GetForegroundWindow();
        let fg_thread = if fg_hwnd.is_null() {
            0
        } else {
            GetWindowThreadProcessId(fg_hwnd, std::ptr::null_mut())
        };

        // Sharing the foreground thread's input queue relaxes the foreground
        // lock for SetForegroundWindow. Always detach afterwards.
        let attached = fg_thread != 0
            && fg_thread != cur_thread
            && AttachThreadInput(cur_thread, fg_thread, 1) != 0;

        BringWindowToTop(target_hwnd);
        let activated = SetForegroundWindow(target_hwnd) != 0;

        if attached {
            let _ = AttachThreadInput(cur_thread, fg_thread, 0);
        }

        if !activated {
            log_message(
                "WARN",
                &format!(
                    "SetForegroundWindow failed for captured target {hwnd_raw:#x} (attached={attached}, fg_thread={fg_thread}, cur_thread={cur_thread})"
                ),
            );
        }
    }
}

/// Confirms the captured target is in the foreground before synthetic keys are sent.
pub(super) fn is_target_foreground() -> bool {
    let Some(prev) = load_previous_window() else {
        return false;
    };
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return false;
        }
        if fg == prev.hwnd as Hwnd {
            return true;
        }
        // Some apps replace their top-level Hwnd between capture and paste.
        // Accepting the captured process is still safe: never another app.
        if prev.pid == 0 {
            return false;
        }
        let mut fg_pid: u32 = 0;
        GetWindowThreadProcessId(fg, &raw mut fg_pid);
        fg_pid != 0 && fg_pid == prev.pid
    }
}

pub(super) fn activate_existing_instance(expected_pid: Option<u32>) -> bool {
    struct SearchCtx {
        expected_pid: Option<u32>,
        found: Hwnd,
    }

    unsafe extern "system" fn enum_proc(hwnd: Hwnd, lparam: isize) -> i32 {
        let ctx = &mut *(lparam as *mut SearchCtx);
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &raw mut pid);
        if let Some(expected) = ctx.expected_pid {
            if pid != expected {
                return 1;
            }
        }
        let mut buf = [0u16; 64];
        let len = GetWindowTextW(
            hwnd,
            buf.as_mut_ptr(),
            i32::try_from(buf.len()).unwrap_or(0),
        );
        if let Ok(len_usize) = usize::try_from(len) {
            if len_usize > 0 {
                let title = String::from_utf16_lossy(&buf[..len_usize]);
                if title == "CipherClipboard" {
                    ctx.found = hwnd;
                    return 0;
                }
            }
        }
        1
    }

    let mut ctx = SearchCtx {
        expected_pid,
        found: std::ptr::null_mut(),
    };
    unsafe {
        EnumWindows(enum_proc, std::ptr::addr_of_mut!(ctx) as isize);
        if ctx.found.is_null() {
            false
        } else {
            // SW_RESTORE on a maximized/snapped window would un-maximize it.
            if IsIconic(ctx.found) != 0 {
                ShowWindow(ctx.found, SW_RESTORE);
            } else {
                ShowWindow(ctx.found, SW_SHOW);
            }
            BringWindowToTop(ctx.found);
            SetForegroundWindow(ctx.found);
            true
        }
    }
}

pub(super) fn show_error_dialog(message: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    extern "system" {
        fn MessageBoxW(hwnd: Hwnd, lpText: *const u16, lpCaption: *const u16, uType: u32) -> i32;
    }

    const MB_OK: u32 = 0x0000_0000;
    const MB_ICONERROR: u32 = 0x0000_0010;

    let msg_wide: Vec<u16> = OsStr::new(message)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let title_wide: Vec<u16> = OsStr::new("CipherClipboard Error")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            msg_wide.as_ptr(),
            title_wide.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// Returns `true` only when all four keyboard events were inserted.
pub(super) fn simulate_paste() -> bool {
    #[allow(non_snake_case)]
    #[repr(C)]
    struct KeybdInput {
        wVk: u16,
        wScan: u16,
        dwFlags: u32,
        time: u32,
        dwExtraInfo: usize,
    }

    // Win32 INPUT union is sized to MOUSEINPUT (32 bytes on x64, 24 bytes on x86),
    // making sizeof(Input) == 40 bytes on x64 (4 + 4 align + 24 KeybdInput + 8 pad)
    // and 28 bytes on x86 (4 + 16 KeybdInput + 8 pad).
    #[repr(C)]
    struct Input {
        r#type: u32,
        u: KeybdInput,
        _pad: [u8; 8],
    }

    const _: () = assert!(
        (cfg!(target_pointer_width = "64") && std::mem::size_of::<Input>() == 40)
            || (cfg!(target_pointer_width = "32") && std::mem::size_of::<Input>() == 28)
    );

    extern "system" {
        fn SendInput(cInputs: u32, pInputs: *const Input, cbSize: i32) -> u32;
    }

    const INPUT_KEYBOARD: u32 = 1;
    const KEYEVENTF_KEYUP: u32 = 0x0002;
    const VK_CONTROL: u16 = 0x11;
    const VK_V: u16 = 0x56;

    unsafe {
        // Raw Input / Windows Terminal-style handlers read hardware scan codes
        // and drop events with wScan == 0 even when wVk is set.
        let hkl = target_keyboard_layout();
        let ctrl_scan = u16::try_from(MapVirtualKeyExW(
            u32::from(VK_CONTROL),
            MAPVK_VK_TO_VSC,
            hkl,
        ))
        .unwrap_or(0);
        let v_scan =
            u16::try_from(MapVirtualKeyExW(u32::from(VK_V), MAPVK_VK_TO_VSC, hkl)).unwrap_or(0);

        let inputs = [
            // Ctrl Down
            Input {
                r#type: INPUT_KEYBOARD,
                u: KeybdInput {
                    wVk: VK_CONTROL,
                    wScan: ctrl_scan,
                    dwFlags: 0,
                    time: 0,
                    dwExtraInfo: 0,
                },
                _pad: [0; 8],
            },
            // V Down
            Input {
                r#type: INPUT_KEYBOARD,
                u: KeybdInput {
                    wVk: VK_V,
                    wScan: v_scan,
                    dwFlags: 0,
                    time: 0,
                    dwExtraInfo: 0,
                },
                _pad: [0; 8],
            },
            // V Up
            Input {
                r#type: INPUT_KEYBOARD,
                u: KeybdInput {
                    wVk: VK_V,
                    wScan: v_scan,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
                _pad: [0; 8],
            },
            // Ctrl Up
            Input {
                r#type: INPUT_KEYBOARD,
                u: KeybdInput {
                    wVk: VK_CONTROL,
                    wScan: ctrl_scan,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
                _pad: [0; 8],
            },
        ];

        let inserted = SendInput(
            4,
            inputs.as_ptr(),
            i32::try_from(std::mem::size_of::<Input>()).unwrap_or(0),
        );
        let ok = inserted == 4;
        if !ok {
            log_message(
                "WARN",
                &format!(
                    "SendInput inserted {inserted}/4 events for auto-paste (possible UIPI block)"
                ),
            );
        }
        ok
    }
}

extern "system" {
    fn RegisterClipboardFormatW(lpszFormat: *const u16) -> u32;
    fn IsClipboardFormatAvailable(format: u32) -> i32;
}

fn cached_clipboard_format(cache: &AtomicU32, name: &str) -> u32 {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let id = cache.load(Ordering::Relaxed);
    if id != 0 {
        return id;
    }
    let wide: Vec<u16> = OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let registered = unsafe { RegisterClipboardFormatW(wide.as_ptr()) };
    if registered != 0 {
        cache.store(registered, Ordering::Relaxed);
    }
    registered
}

fn is_clipboard_history_excluded(history_id: u32) -> bool {
    #[link(name = "user32")]
    extern "system" {
        fn OpenClipboard(hWndNewOwner: Hwnd) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(uFormat: u32) -> *mut std::ffi::c_void;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalLock(hMem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn GlobalUnlock(hMem: *mut std::ffi::c_void) -> i32;
        fn GlobalSize(hMem: *mut std::ffi::c_void) -> usize;
    }

    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }
        let hmem = GetClipboardData(history_id);
        let mut excluded = false;
        if !hmem.is_null() && GlobalSize(hmem) >= std::mem::size_of::<u32>() {
            let ptr = GlobalLock(hmem).cast::<u32>();
            if !ptr.is_null() {
                excluded = *ptr == 0;
                let _ = GlobalUnlock(hmem);
            }
        }
        let _ = CloseClipboard();
        excluded
    }
}

pub(super) fn clipboard_change_token() -> Option<u64> {
    #[link(name = "user32")]
    extern "system" {
        fn GetClipboardSequenceNumber() -> u32;
    }
    let seq = unsafe { GetClipboardSequenceNumber() };
    (seq != 0).then(|| u64::from(seq))
}

pub(super) fn is_clipboard_concealed() -> bool {
    static EXCLUDE_FMT: AtomicU32 = AtomicU32::new(0);
    static IGNORE_FMT: AtomicU32 = AtomicU32::new(0);
    static HISTORY_FMT: AtomicU32 = AtomicU32::new(0);

    let exclude_id =
        cached_clipboard_format(&EXCLUDE_FMT, "ExcludeClipboardContentFromMonitorProcessing");
    if exclude_id != 0 && unsafe { IsClipboardFormatAvailable(exclude_id) } != 0 {
        return true;
    }

    let ignore_id = cached_clipboard_format(&IGNORE_FMT, "Clipboard Viewer Ignore");
    if ignore_id != 0 && unsafe { IsClipboardFormatAvailable(ignore_id) } != 0 {
        return true;
    }

    // Windows 10/11 Clipboard History API: apps set CanIncludeInClipboardHistory = DWORD(0)
    // to mark sensitive items as excluded from clipboard history.
    let history_id = cached_clipboard_format(&HISTORY_FMT, "CanIncludeInClipboardHistory");
    history_id != 0
        && unsafe { IsClipboardFormatAvailable(history_id) } != 0
        && is_clipboard_history_excluded(history_id)
}

/// Appends Windows privacy clipboard formats without clearing the text just written:
/// - `ExcludeClipboardContentFromMonitorProcessing` (prevents clipboard monitors)
/// - `CanIncludeInClipboardHistory` = 0 (blocks Win+V local history)
/// - `CanUploadClipboardContentToCloud` = 0 (blocks Windows Cloud Clipboard sync)
pub(super) fn mark_clipboard_concealed() {
    #[link(name = "user32")]
    extern "system" {
        fn OpenClipboard(hWndNewOwner: Hwnd) -> i32;
        fn CloseClipboard() -> i32;
        fn SetClipboardData(uFormat: u32, hMem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalAlloc(uFlags: u32, dwBytes: usize) -> *mut std::ffi::c_void;
        fn GlobalLock(hMem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn GlobalUnlock(hMem: *mut std::ffi::c_void) -> i32;
        fn GlobalFree(hMem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }

    const GMEM_MOVEABLE: u32 = 0x0002;

    static EXCLUDE_FMT: AtomicU32 = AtomicU32::new(0);
    static HISTORY_FMT: AtomicU32 = AtomicU32::new(0);
    static CLOUD_FMT: AtomicU32 = AtomicU32::new(0);

    unsafe fn set_zero_dword_format(cache: &AtomicU32, name: &str) {
        let fmt = cached_clipboard_format(cache, name);
        if fmt == 0 {
            return;
        }
        let hmem = GlobalAlloc(GMEM_MOVEABLE, std::mem::size_of::<u32>());
        if hmem.is_null() {
            return;
        }
        let ptr = GlobalLock(hmem).cast::<u32>();
        if ptr.is_null() {
            let _ = GlobalFree(hmem);
            return;
        }
        *ptr = 0;
        let _ = GlobalUnlock(hmem);
        if SetClipboardData(fmt, hmem).is_null() {
            let _ = GlobalFree(hmem);
        }
    }

    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return;
        }
        set_zero_dword_format(&EXCLUDE_FMT, "ExcludeClipboardContentFromMonitorProcessing");
        set_zero_dword_format(&HISTORY_FMT, "CanIncludeInClipboardHistory");
        set_zero_dword_format(&CLOUD_FMT, "CanUploadClipboardContentToCloud");
        let _ = CloseClipboard();
    }
}

/// Returns `true` when the active target thread's Windows keyboard layout produces `ù`
/// for the physical key in the US `Quote` position (French/Belgian AZERTY layouts).
pub(super) fn quote_key_is_ugrave() -> bool {
    unsafe {
        let hkl = target_keyboard_layout();
        if hkl.is_null() {
            return false;
        }
        let vk = MapVirtualKeyExW(0x0028, MAPVK_VSC_TO_VK, hkl);
        if vk == 0 {
            return false;
        }
        let key_state = [0u8; 256];
        let mut buf = [0u16; 4];
        let rc = ToUnicodeEx(
            vk,
            0x0028,
            key_state.as_ptr(),
            buf.as_mut_ptr(),
            i32::try_from(buf.len()).unwrap_or(0),
            0,
            hkl,
        );
        if let Ok(rc_usize) = usize::try_from(rc) {
            if rc_usize > 0 {
                let text = String::from_utf16_lossy(&buf[..rc_usize]);
                return text.chars().any(|c| c == 'ù' || c == 'Ù');
            }
        }
        false
    }
}

unsafe fn captured_target_pid() -> u32 {
    let Some(prev) = load_previous_window() else {
        return 0;
    };
    let hwnd = prev.hwnd as Hwnd;
    if IsWindow(hwnd) != 0 {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &raw mut pid);
        if pid != 0 && pid != GetCurrentProcessId() {
            return pid;
        }
    }
    if prev.pid != 0 && prev.pid != GetCurrentProcessId() {
        prev.pid
    } else {
        0
    }
}

/// True when the captured target runs at a higher mandatory integrity level
/// than us (normally: it runs as Administrator). UIPI silently discards the
/// injected Ctrl+V across that boundary, so auto-paste must be skipped.
pub(super) fn target_is_elevated() -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(
            dwDesiredAccess: u32,
            bInheritHandle: i32,
            dwProcessId: u32,
        ) -> *mut std::ffi::c_void;
        fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn OpenProcessToken(
            ProcessHandle: *mut std::ffi::c_void,
            DesiredAccess: u32,
            TokenHandle: *mut *mut std::ffi::c_void,
        ) -> i32;
        fn GetTokenInformation(
            TokenHandle: *mut std::ffi::c_void,
            TokenInformationClass: i32,
            TokenInformation: *mut std::ffi::c_void,
            TokenInformationLength: u32,
            ReturnLength: *mut u32,
        ) -> i32;
        fn GetSidSubAuthorityCount(pSid: *mut std::ffi::c_void) -> *mut u8;
        fn GetSidSubAuthority(pSid: *mut std::ffi::c_void, nSubAuthority: u32) -> *mut u32;
    }

    // TOKEN_MANDATORY_LABEL = SID_AND_ATTRIBUTES followed inline by the SID bytes.
    // Allocate a properly aligned 256-byte buffer so GetTokenInformation can store
    // both the header and the inline integrity SID without ERROR_INSUFFICIENT_BUFFER.
    #[allow(dead_code)]
    #[repr(C)]
    struct SidAndAttributes {
        sid: *mut std::ffi::c_void,
        attributes: u32,
    }

    #[repr(C, align(8))]
    struct TokenIntegrityBuf([u8; 256]);

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_INTEGRITY_LEVEL_CLASS: i32 = 25;
    const SECURITY_MANDATORY_MEDIUM_RID: u32 = 0x2000;

    unsafe fn integrity_level(pid: u32) -> Option<u32> {
        if pid == 0 {
            return None;
        }
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if proc.is_null() {
            return None;
        }
        let mut token: *mut std::ffi::c_void = std::ptr::null_mut();
        let opened = OpenProcessToken(proc, TOKEN_QUERY, &raw mut token) != 0 && !token.is_null();
        let mut level = None;
        if opened {
            let mut buf = TokenIntegrityBuf([0u8; 256]);
            let mut ret_len = 0u32;
            let ok = GetTokenInformation(
                token,
                TOKEN_INTEGRITY_LEVEL_CLASS,
                buf.0.as_mut_ptr().cast::<std::ffi::c_void>(),
                u32::try_from(buf.0.len()).unwrap_or(256),
                &raw mut ret_len,
            );
            if ok != 0 {
                let info = &*std::ptr::addr_of!(buf).cast::<SidAndAttributes>();
                if !info.sid.is_null() {
                    let count = GetSidSubAuthorityCount(info.sid);
                    if !count.is_null() && *count > 0 {
                        let rid = GetSidSubAuthority(info.sid, u32::from(*count - 1));
                        if !rid.is_null() {
                            level = Some(*rid);
                        }
                    }
                }
            }
            let _ = CloseHandle(token);
        }
        let _ = CloseHandle(proc);
        level
    }

    unsafe {
        let target_pid = captured_target_pid();
        if target_pid == 0 {
            return false;
        }
        let self_level =
            integrity_level(GetCurrentProcessId()).unwrap_or(SECURITY_MANDATORY_MEDIUM_RID);
        // Protected / higher-integrity processes often refuse TOKEN_QUERY.
        // Fail closed: asking the user to paste manually is safer than a
        // silent drop by UIPI.
        integrity_level(target_pid).is_none_or(|level| level > self_level)
    }
}

pub(super) fn set_title_bar_dark_mode(is_dark: bool) {
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: Hwnd,
            dwAttribute: u32,
            pvAttribute: *const std::ffi::c_void,
            cbAttribute: u32,
        ) -> i32;
    }
    const DWMWA_USE_IMMERSIVE_DARK_MODE_BEFORE_20H1: u32 = 19;
    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;

    let Some(target_hwnd) = find_main_window() else {
        return;
    };

    unsafe {
        let value: i32 = i32::from(is_dark);
        if DwmSetWindowAttribute(
            target_hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            std::ptr::addr_of!(value).cast::<std::ffi::c_void>(),
            4,
        ) != 0
        {
            let _ = DwmSetWindowAttribute(
                target_hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE_BEFORE_20H1,
                std::ptr::addr_of!(value).cast::<std::ffi::c_void>(),
                4,
            );
        }
    }
}

#[repr(C)]
struct DataBlob {
    cb_data: u32,
    pb_data: *mut u8,
}

#[link(name = "crypt32")]
extern "system" {
    fn CryptProtectData(
        p_data_in: *const DataBlob,
        sz_data_descr: *const u16,
        p_optional_entropy: *const DataBlob,
        pv_reserved: *mut std::ffi::c_void,
        p_prompt_struct: *mut std::ffi::c_void,
        dw_flags: u32,
        p_data_out: *mut DataBlob,
    ) -> i32;

    fn CryptUnprotectData(
        p_data_in: *const DataBlob,
        ppsz_data_descr: *mut *mut u16,
        p_optional_entropy: *const DataBlob,
        pv_reserved: *mut std::ffi::c_void,
        p_prompt_struct: *mut std::ffi::c_void,
        dw_flags: u32,
        p_data_out: *mut DataBlob,
    ) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn LocalFree(h_mem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
}

const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x01;
const DPAPI_ENTROPY: &[u8] = b"CipherClipboard-DPAPI-v1";

pub(super) fn dpapi_protect(plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let in_len = u32::try_from(plaintext.len())
        .map_err(|_| "DPAPI plaintext exceeds u32::MAX bytes".to_string())?;
    let entropy_len = u32::try_from(DPAPI_ENTROPY.len())
        .map_err(|_| "DPAPI entropy length invalid".to_string())?;

    let in_blob = DataBlob {
        cb_data: in_len,
        pb_data: plaintext.as_ptr().cast_mut(),
    };
    let entropy_blob = DataBlob {
        cb_data: entropy_len,
        pb_data: DPAPI_ENTROPY.as_ptr().cast_mut(),
    };
    let mut out_blob = DataBlob {
        cb_data: 0,
        pb_data: std::ptr::null_mut(),
    };

    let ok = unsafe {
        CryptProtectData(
            &raw const in_blob,
            std::ptr::null(),
            &raw const entropy_blob,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &raw mut out_blob,
        )
    };
    if ok == 0 || out_blob.pb_data.is_null() {
        return Err("Windows DPAPI CryptProtectData failed".to_string());
    }

    let out_len = usize::try_from(out_blob.cb_data).unwrap_or(0);
    let ciphertext = unsafe {
        let slice = std::slice::from_raw_parts(out_blob.pb_data, out_len);
        let copied = slice.to_vec();
        let _ = LocalFree(out_blob.pb_data.cast::<std::ffi::c_void>());
        copied
    };
    Ok(ciphertext)
}

pub(super) fn dpapi_unprotect(ciphertext: &[u8]) -> Result<zeroize::Zeroizing<Vec<u8>>, String> {
    use zeroize::Zeroize;

    let in_len = u32::try_from(ciphertext.len())
        .map_err(|_| "DPAPI ciphertext exceeds u32::MAX bytes".to_string())?;
    let entropy_len = u32::try_from(DPAPI_ENTROPY.len())
        .map_err(|_| "DPAPI entropy length invalid".to_string())?;

    let in_blob = DataBlob {
        cb_data: in_len,
        pb_data: ciphertext.as_ptr().cast_mut(),
    };
    let entropy_blob = DataBlob {
        cb_data: entropy_len,
        pb_data: DPAPI_ENTROPY.as_ptr().cast_mut(),
    };
    let mut out_blob = DataBlob {
        cb_data: 0,
        pb_data: std::ptr::null_mut(),
    };

    let ok = unsafe {
        CryptUnprotectData(
            &raw const in_blob,
            std::ptr::null_mut(),
            &raw const entropy_blob,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &raw mut out_blob,
        )
    };
    if ok == 0 || out_blob.pb_data.is_null() {
        return Err("Windows DPAPI CryptUnprotectData failed".to_string());
    }

    let out_len = usize::try_from(out_blob.cb_data).unwrap_or(0);
    let plaintext = unsafe {
        let slice = std::slice::from_raw_parts_mut(out_blob.pb_data, out_len);
        let copied = zeroize::Zeroizing::new(slice.to_vec());
        slice.zeroize();
        let _ = LocalFree(out_blob.pb_data.cast::<std::ffi::c_void>());
        copied
    };
    Ok(plaintext)
}
