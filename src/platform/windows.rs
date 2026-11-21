#![allow(unsafe_code)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{CloseHandle, GlobalFree, LocalFree, HANDLE, HWND, LPARAM};
use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};
use windows_sys::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenIntegrityLevel,
    TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows_sys::Win32::System::SystemServices::SECURITY_MANDATORY_MEDIUM_RID;
use windows_sys::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, MapVirtualKeyExW, SendInput, ToUnicodeEx, HKL, INPUT, INPUT_0,
    INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MAPVK_VSC_TO_VK, VK_CONTROL,
    VK_V,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    IsIconic, IsWindow, MessageBoxW, PeekMessageW, SetForegroundWindow, ShowWindow, MB_ICONERROR,
    MB_OK, MSG, PM_NOREMOVE, SW_RESTORE, SW_SHOW,
};

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

/// Resolves the keyboard layout (HKL) for the thread whose layout the user is
/// currently typing with, preferring the foreground window's thread (or the
/// captured target window when our own window is foreground) over our UI thread.
///
/// # Safety
/// Call only on Windows after the Win32 subsystem is initialized. The returned
/// `HKL` is a borrowed OS handle; callers must not retain it across layout changes.
unsafe fn target_keyboard_layout() -> HKL {
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
        let hwnd = prev.hwnd as HWND;
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

fn find_main_window() -> Option<HWND> {
    // SAFETY: `EnumWindows` invokes this synchronously with a valid HWND; `lparam` points to the caller's local `target_hwnd`, which stays alive for the whole enumeration.
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
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
                        let out = lparam as *mut HWND;
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

    let mut target_hwnd: HWND = std::ptr::null_mut();
    // SAFETY: `enum_proc` is a valid `extern "system"` callback and `target_hwnd` is a live local for the synchronous `EnumWindows` call.
    unsafe {
        EnumWindows(
            Some(enum_proc),
            std::ptr::addr_of_mut!(target_hwnd) as LPARAM,
        );
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

    // SAFETY: The HWND came from enumeration; window managers reject stale handles without dereferencing them, local PID out-pointers are valid, and any input attachment is detached.
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
    // SAFETY: `GetForegroundWindow` may be null and is checked; the PID out-pointer references a live local.
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
    // SAFETY: The foreground HWND is null-checked; the PID out-pointer references a live local and is only stored when non-zero and not our process.
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

    // SAFETY: The target HWND was validated with `IsWindow`; `MSG` is a live zeroed local, foreground PID out-pointers are valid, and thread input is detached on every path.
    unsafe {
        let target_hwnd = hwnd_raw as HWND;
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
    // SAFETY: The foreground HWND is null-checked; the PID out-pointer is a live local and the comparison uses only scalar IDs.
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return false;
        }
        if fg == prev.hwnd as HWND {
            return true;
        }
        // Some apps replace their top-level HWND between capture and paste.
        // Accepting the captured process is still safe: never another app.
        if prev.pid == 0 {
            return false;
        }
        let mut fg_pid: u32 = 0;
        GetWindowThreadProcessId(fg, &raw mut fg_pid);
        fg_pid != 0 && fg_pid == prev.pid
    }
}

pub(crate) fn activate_existing_instance(expected_pid: Option<u32>) -> bool {
    struct SearchCtx {
        expected_pid: Option<u32>,
        found: HWND,
    }

    // SAFETY: `EnumWindows` invokes this callback synchronously on the calling thread, so `lparam` is non-null, correctly aligned, and uniquely borrows the caller's live stack-local `SearchCtx` for the duration of the call; no callback state is retained or shared across threads. The title buffer is indexed only by the length `GetWindowTextW` reports.
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
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
    // SAFETY: `enum_proc` is a valid callback whose `lparam` uniquely borrows this stack-local `ctx` for the synchronous `EnumWindows` call; `addr_of_mut!` is used to form the raw pointer without creating an aliasing reference. Stale HWNDs are passed only to APIs that fail safely.
    unsafe {
        EnumWindows(Some(enum_proc), std::ptr::addr_of_mut!(ctx) as LPARAM);
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

pub(crate) fn show_error_dialog(message: &str) {
    let msg_wide: Vec<u16> = OsStr::new(message)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let title_wide: Vec<u16> = OsStr::new("CipherClipboard Error")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: Both wide buffers are NUL-terminated by `chain(once(0))` and remain alive for the call; a null owner and the fixed flags are valid for `MessageBoxW`.
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
    const fn key_event(vk: u16, scan: u16, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    // SAFETY: The keyboard layout query is safe Win32; virtual-key scans are checked before use, every `INPUT` field is initialized, and `SendInput`'s count and `cbSize` match the array.
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
            key_event(VK_CONTROL, ctrl_scan, 0),
            key_event(VK_V, v_scan, 0),
            key_event(VK_V, v_scan, KEYEVENTF_KEYUP),
            key_event(VK_CONTROL, ctrl_scan, KEYEVENTF_KEYUP),
        ];

        let inserted = SendInput(
            4,
            inputs.as_ptr(),
            i32::try_from(std::mem::size_of::<INPUT>()).unwrap_or(0),
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

fn cached_clipboard_format(cache: &AtomicU32, name: &str) -> u32 {
    let id = cache.load(Ordering::Relaxed);
    if id != 0 {
        return id;
    }
    let wide: Vec<u16> = OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` is NUL-terminated and alive for the call; `RegisterClipboardFormatW` returns 0 on failure, which the caller handles.
    let registered = unsafe { RegisterClipboardFormatW(wide.as_ptr()) };
    if registered != 0 {
        cache.store(registered, Ordering::Relaxed);
    }
    registered
}

fn is_clipboard_history_excluded(history_id: u32) -> bool {
    // SAFETY: The clipboard is opened successfully, the data handle is non-null and large enough, the locked pointer is checked before dereference, and both the lock and clipboard are released.
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
    // SAFETY: `GetClipboardSequenceNumber` takes no pointers and only reads a Win32 counter.
    let seq = unsafe { GetClipboardSequenceNumber() };
    (seq != 0).then(|| u64::from(seq))
}

pub(super) fn is_clipboard_concealed() -> bool {
    static EXCLUDE_FMT: AtomicU32 = AtomicU32::new(0);
    static IGNORE_FMT: AtomicU32 = AtomicU32::new(0);
    static HISTORY_FMT: AtomicU32 = AtomicU32::new(0);

    let exclude_id =
        cached_clipboard_format(&EXCLUDE_FMT, super::WINDOWS_CONCEALED_DETECT_FORMATS[0]);
    // SAFETY: The format atom was registered earlier in this call (0 is guarded); `IsClipboardFormatAvailable` only reads a global format table.
    let exclude_available =
        exclude_id != 0 && unsafe { IsClipboardFormatAvailable(exclude_id) } != 0;
    if exclude_available {
        return true;
    }

    let ignore_id =
        cached_clipboard_format(&IGNORE_FMT, super::WINDOWS_CONCEALED_DETECT_FORMATS[1]);
    // SAFETY: The format atom was registered earlier in this call (0 is guarded); `IsClipboardFormatAvailable` only reads a global format table.
    let ignore_available = ignore_id != 0 && unsafe { IsClipboardFormatAvailable(ignore_id) } != 0;
    if ignore_available {
        return true;
    }

    // Windows 10/11 Clipboard History API: apps set CanIncludeInClipboardHistory = DWORD(0)
    // to mark sensitive items as excluded from clipboard history.
    let history_id =
        cached_clipboard_format(&HISTORY_FMT, super::WINDOWS_CONCEALED_DETECT_FORMATS[2]);
    // SAFETY: The format atom was registered earlier in this call (0 is guarded); `IsClipboardFormatAvailable` only reads a global format table.
    let history_excluded = history_id != 0
        && unsafe { IsClipboardFormatAvailable(history_id) } != 0
        && is_clipboard_history_excluded(history_id);

    super::windows_concealed_detected(exclude_available, ignore_available, history_excluded)
}

/// Appends Windows privacy clipboard formats without clearing the text just written:
/// - `ExcludeClipboardContentFromMonitorProcessing` (prevents clipboard monitors)
/// - `CanIncludeInClipboardHistory` = 0 (blocks Win+V local history)
/// - `CanUploadClipboardContentToCloud` = 0 (blocks Windows Cloud Clipboard sync)
pub(super) fn mark_clipboard_concealed() {
    static EXCLUDE_FMT: AtomicU32 = AtomicU32::new(0);
    static HISTORY_FMT: AtomicU32 = AtomicU32::new(0);
    static CLOUD_FMT: AtomicU32 = AtomicU32::new(0);

    /// Registers a zero DWORD clipboard format, reusing a cached format ID.
    ///
    /// # Safety
    /// The caller must hold the clipboard open; allocation, locking, and
    /// `SetClipboardData` are checked and any unowned global is freed.
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

    // SAFETY: `OpenClipboard` is checked, zero-DWORD helpers run only while the clipboard is open, and `CloseClipboard` always runs.
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return;
        }
        set_zero_dword_format(&EXCLUDE_FMT, super::WINDOWS_CONCEALED_EMIT_FORMATS[0]);
        set_zero_dword_format(&HISTORY_FMT, super::WINDOWS_CONCEALED_EMIT_FORMATS[1]);
        set_zero_dword_format(&CLOUD_FMT, super::WINDOWS_CONCEALED_EMIT_FORMATS[2]);
        let _ = CloseClipboard();
    }
}

/// Returns `true` when the active target thread's Windows keyboard layout produces `ù`
/// for the physical key in the US `Quote` position (French/Belgian AZERTY layouts).
pub(super) fn quote_key_is_ugrave() -> bool {
    // SAFETY: The layout query is safe Win32; the key-state and output buffers are initialized and sized, and `ToUnicodeEx` output is bounded by `buf.len()` and checked before slicing.
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

/// Returns the PID of the captured foreground target, or `0` if unavailable.
///
/// # Safety
/// The captured HWND is validated with `IsWindow` before the PID query; the
/// out-pointer is a live local and only non-self PIDs are returned.
unsafe fn captured_target_pid() -> u32 {
    let Some(prev) = load_previous_window() else {
        return 0;
    };
    let hwnd = prev.hwnd as HWND;
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
pub(crate) fn target_is_elevated() -> bool {
    // TOKEN_MANDATORY_LABEL = SID_AND_ATTRIBUTES followed inline by the SID bytes.
    // Allocate a properly aligned 256-byte buffer so GetTokenInformation can store
    // both the header and the inline integrity SID without ERROR_INSUFFICIENT_BUFFER.
    #[repr(C, align(8))]
    struct TokenIntegrityBuf([u8; 256]);

    /// Reads the mandatory integrity level of `pid`, or `None` if it cannot be queried.
    ///
    /// # Safety
    /// All Win32 handle calls are null-checked and closed; `ret_len` is validated
    /// to be at least the `TOKEN_MANDATORY_LABEL` header and no larger than the
    /// 8-byte-aligned 256-byte buffer before the cast, and SID pointers are
    /// checked before dereference.
    unsafe fn integrity_level(pid: u32) -> Option<u32> {
        if pid == 0 {
            return None;
        }
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if proc.is_null() {
            return None;
        }
        let mut token: HANDLE = std::ptr::null_mut();
        let opened = OpenProcessToken(proc, TOKEN_QUERY, &raw mut token) != 0 && !token.is_null();
        let mut level = None;
        if opened {
            let mut buf = TokenIntegrityBuf([0u8; 256]);
            let mut ret_len = 0u32;
            let buffer_len = u32::try_from(buf.0.len()).unwrap_or(256);
            let ok = GetTokenInformation(
                token,
                TokenIntegrityLevel,
                buf.0.as_mut_ptr().cast::<std::ffi::c_void>(),
                buffer_len,
                &raw mut ret_len,
            );
            let min_len =
                u32::try_from(std::mem::size_of::<TOKEN_MANDATORY_LABEL>()).unwrap_or(u32::MAX);
            if ok != 0 && (ret_len < min_len || ret_len > buffer_len) {
                crate::logging::log_message(
                    "WARN",
                    &format!("Unexpected TOKEN_MANDATORY_LABEL length {ret_len}; skipping UIPI elevation check"),
                );
            }
            if ok != 0 && ret_len >= min_len && ret_len <= buffer_len {
                let info = &*std::ptr::addr_of!(buf).cast::<TOKEN_MANDATORY_LABEL>();
                if !info.Label.Sid.is_null() {
                    let count = GetSidSubAuthorityCount(info.Label.Sid);
                    if !count.is_null() && *count > 0 {
                        let rid = GetSidSubAuthority(info.Label.Sid, u32::from(*count - 1));
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

    // SAFETY: Both helper calls honor their safety contracts and query handles internally; a failed integrity query fails closed.
    unsafe {
        let target_pid = captured_target_pid();
        if target_pid == 0 {
            return false;
        }
        let self_level = integrity_level(GetCurrentProcessId())
            .unwrap_or(SECURITY_MANDATORY_MEDIUM_RID.cast_unsigned());
        // Protected / higher-integrity processes often refuse TOKEN_QUERY.
        // Fail closed: asking the user to paste manually is safer than a
        // silent drop by UIPI.
        super::uipi_should_skip_paste(target_pid, integrity_level(target_pid), self_level)
    }
}

pub(super) fn set_title_bar_dark_mode(is_dark: bool) {
    const DWMWA_USE_IMMERSIVE_DARK_MODE_BEFORE_20H1: u32 = 19;

    let Some(target_hwnd) = find_main_window() else {
        return;
    };

    // SAFETY: The HWND came from enumeration, the value is a live local i32, and the pointer/size passed to `DwmSetWindowAttribute` match that storage.
    unsafe {
        let value: i32 = i32::from(is_dark);
        let attr = DWMWA_USE_IMMERSIVE_DARK_MODE.cast_unsigned();
        if DwmSetWindowAttribute(
            target_hwnd,
            attr,
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

const DPAPI_ENTROPY: &[u8] = b"CipherClipboard-DPAPI-v1";

/// Encrypts `plaintext` with DPAPI using the application entropy.
///
/// # Errors
/// Returns an error if the input is too large for DPAPI or `CryptProtectData` fails.
pub(crate) fn dpapi_protect(plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let in_len = u32::try_from(plaintext.len())
        .map_err(|_| "DPAPI plaintext exceeds u32::MAX bytes".to_string())?;
    let entropy_len = u32::try_from(DPAPI_ENTROPY.len())
        .map_err(|_| "DPAPI entropy length invalid".to_string())?;

    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: in_len,
        pbData: plaintext.as_ptr().cast_mut(),
    };
    let entropy_blob = CRYPT_INTEGER_BLOB {
        cbData: entropy_len,
        pbData: DPAPI_ENTROPY.as_ptr().cast_mut(),
    };
    let mut out_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    // SAFETY: `pDataIn` and `pEntropy` are input-only per the DPAPI contract; the `.cast_mut()` calls only satisfy `CRYPT_INTEGER_BLOB`'s `*mut u8` field type and are never written through. Both input slices live for the whole call with checked lengths, output is initialized null, and the out-blob buffer is owned by us until `LocalFree`.
    let ok = unsafe {
        CryptProtectData(
            &raw const in_blob,
            std::ptr::null(),
            &raw const entropy_blob,
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &raw mut out_blob,
        )
    };
    if ok == 0 || out_blob.pbData.is_null() {
        return Err("Windows DPAPI CryptProtectData failed".to_string());
    }

    let out_len = usize::try_from(out_blob.cbData).unwrap_or(0);
    // SAFETY: The output pointer was checked non-null and `out_len` comes from `cbData`; the bytes are copied before the owned buffer is freed once.
    let ciphertext = unsafe {
        let slice = std::slice::from_raw_parts(out_blob.pbData, out_len);
        let copied = slice.to_vec();
        let _ = LocalFree(out_blob.pbData.cast::<std::ffi::c_void>());
        copied
    };
    Ok(ciphertext)
}

/// Decrypts DPAPI ciphertext produced by [`dpapi_protect`].
///
/// # Errors
/// Returns an error if the input is too large for DPAPI, `CryptUnprotectData`
/// fails, or the ciphertext was not produced with the application entropy.
pub(crate) fn dpapi_unprotect(ciphertext: &[u8]) -> Result<zeroize::Zeroizing<Vec<u8>>, String> {
    use zeroize::Zeroize;

    let in_len = u32::try_from(ciphertext.len())
        .map_err(|_| "DPAPI ciphertext exceeds u32::MAX bytes".to_string())?;
    let entropy_len = u32::try_from(DPAPI_ENTROPY.len())
        .map_err(|_| "DPAPI entropy length invalid".to_string())?;

    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: in_len,
        pbData: ciphertext.as_ptr().cast_mut(),
    };
    let entropy_blob = CRYPT_INTEGER_BLOB {
        cbData: entropy_len,
        pbData: DPAPI_ENTROPY.as_ptr().cast_mut(),
    };
    let mut out_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    // SAFETY: `pDataIn` and `pEntropy` are input-only per the DPAPI contract; the `.cast_mut()` calls only satisfy `CRYPT_INTEGER_BLOB`'s `*mut u8` field type and are never written through. Both input slices live for the whole call with checked lengths, output is initialized null, and the plaintext buffer is owned by us until `LocalFree`.
    let ok = unsafe {
        CryptUnprotectData(
            &raw const in_blob,
            std::ptr::null_mut(),
            &raw const entropy_blob,
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &raw mut out_blob,
        )
    };
    if ok == 0 || out_blob.pbData.is_null() {
        return Err("Windows DPAPI CryptUnprotectData failed".to_string());
    }

    let out_len = usize::try_from(out_blob.cbData).unwrap_or(0);
    // SAFETY: The output pointer was checked non-null and `out_len` comes from `cbData`; bytes are copied into `Zeroizing`, zeroed, then the owned buffer is freed once.
    let plaintext = unsafe {
        let slice = std::slice::from_raw_parts_mut(out_blob.pbData, out_len);
        let copied = zeroize::Zeroizing::new(slice.to_vec());
        slice.zeroize();
        let _ = LocalFree(out_blob.pbData.cast::<std::ffi::c_void>());
        copied
    };
    Ok(plaintext)
}
