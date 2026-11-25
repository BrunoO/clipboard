#![allow(unsafe_code)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, GlobalFree, LocalFree, ERROR_CLASS_ALREADY_EXISTS, HANDLE, HWND,
    LPARAM, LRESULT, POINT, WAIT_OBJECT_0, WPARAM,
};
use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};
use windows_sys::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, IsValidSid,
    TokenIntegrityLevel, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows_sys::Win32::System::SystemServices::SECURITY_MANDATORY_MEDIUM_RID;
use windows_sys::Win32::System::Threading::{
    AttachThreadInput, CreateEventW, GetCurrentProcessId, GetCurrentThreadId, OpenEventW,
    OpenProcess, OpenProcessToken, SetEvent, WaitForMultipleObjects, EVENT_MODIFY_STATE, INFINITE,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, MapVirtualKeyExW, SendInput, ToUnicodeEx, HKL, INPUT, INPUT_0,
    INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MAPVK_VSC_TO_VK, VK_CONTROL,
    VK_V,
};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION,
    NINF_KEY, NIN_SELECT, NOTIFYICONDATAW, NOTIFYICONDATAW_0, NOTIFYICON_VERSION_4,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, BringWindowToTop, ChangeWindowMessageFilterEx, CreateIcon, CreatePopupMenu,
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow, EnumWindows,
    GetCursorPos, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindow, MessageBoxW, PeekMessageW, PostMessageW, RegisterClassW, RegisterWindowMessageW,
    SetForegroundWindow, SetLayeredWindowAttributes, SetWindowPos, ShowWindow, TrackPopupMenuEx,
    HICON, HMENU, HWND_TOPMOST, LWA_ALPHA, MB_ICONERROR, MB_OK, MF_SEPARATOR, MF_STRING, MSG,
    MSGFLT_ALLOW, PM_NOREMOVE, SWP_SHOWWINDOW, SW_HIDE, SW_RESTORE, SW_SHOW, TPM_BOTTOMALIGN,
    TPM_LEFTALIGN, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_CONTEXTMENU,
    WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NCDESTROY, WM_NULL, WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_TOOLWINDOW, WS_POPUP,
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
    // SAFETY: `GetForegroundWindow` is null-checked and compared against the main
    // `CipherClipboard` HWND from `find_main_window`, so hidden helper windows
    // (`tray-icon`, hotkey, tray menu host) owned by our process do not trigger
    // unintended focus-reopen wakes.
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return false;
        }
        find_main_window().is_some_and(|main_hwnd| fg == main_hwnd)
    }
}

const WM_TRAY_CALLBACK: u32 = WM_APP + 1;
const TRAY_ICON_ID: u32 = 1;
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;
const TRAY_CMD_SHOW: i32 = 1;
const TRAY_CMD_SETTINGS: i32 = 2;
const TRAY_CMD_QUIT: i32 = 3;
const TRAY_CMD_SHOW_USIZE: usize = 1;
const TRAY_CMD_SETTINGS_USIZE: usize = 2;
const TRAY_CMD_QUIT_USIZE: usize = 3;

struct TrayState {
    tray_tx: std::sync::mpsc::Sender<crate::tray::TrayAction>,
    ctx: eframe::egui::Context,
    hicon: isize,
    using_v4: bool,
}

static TRAY_STATE: Mutex<Option<TrayState>> = Mutex::new(None);
static TRAY_ICON_LIVE: AtomicBool = AtomicBool::new(false);
static TRAY_MENU_OPEN: AtomicBool = AtomicBool::new(false);
static TRAY_TASKBAR_CREATED_MSG: AtomicU32 = AtomicU32::new(0);

struct MenuOpenGuard;

impl Drop for MenuOpenGuard {
    fn drop(&mut self) {
        TRAY_MENU_OPEN.store(false, Ordering::Release);
    }
}

fn wide_null(text: &str) -> Vec<u16> {
    OsStr::new(text)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn create_tray_hicon(rgba: &[u8], width: u32, height: u32) -> Result<HICON, String> {
    let w = i32::try_from(width).map_err(|_| "Tray icon width overflow".to_string())?;
    let h = i32::try_from(height).map_err(|_| "Tray icon height overflow".to_string())?;
    let uw = usize::try_from(width).map_err(|_| "Tray icon width overflow".to_string())?;
    let uh = usize::try_from(height).map_err(|_| "Tray icon height overflow".to_string())?;
    let expected_len = uw
        .checked_mul(uh)
        .and_then(|px| px.checked_mul(4))
        .ok_or_else(|| "Tray icon buffer size overflow".to_string())?;
    if rgba.len() != expected_len {
        return Err(format!(
            "Tray icon RGBA buffer mismatch: expected {expected_len}, got {}",
            rgba.len()
        ));
    }

    let mut bgra = Vec::with_capacity(expected_len);
    for px in rgba.chunks_exact(4) {
        bgra.push(px[2]); // B
        bgra.push(px[1]); // G
        bgra.push(px[0]); // R
        bgra.push(px[3]); // A
    }

    // 1bpp AND mask, word-aligned rows, zeroed so Windows uses the 32-bpp alpha channel.
    let row_bytes = uw.div_ceil(16) * 2;
    let and_len = row_bytes
        .checked_mul(uh)
        .ok_or_else(|| "Tray icon mask size overflow".to_string())?;
    let and_mask = vec![0u8; and_len];

    // SAFETY: `and_mask` and `bgra` are live buffers sized for `(w, h)` at 32bpp.
    let hicon = unsafe {
        CreateIcon(
            std::ptr::null_mut(),
            w,
            h,
            1,
            32,
            and_mask.as_ptr(),
            bgra.as_ptr(),
        )
    };
    if hicon.is_null() {
        // SAFETY: `GetLastError` reads the calling thread's last Win32 error code.
        let err = unsafe { GetLastError() };
        return Err(format!("CreateIcon failed for tray icon (err={err})"));
    }
    Ok(hicon)
}

fn create_tray_window() -> Result<HWND, String> {
    let class_name = wide_null("CipherClipboardTrayWindow");
    let window_title = wide_null("CipherClipboardTrayWindow");

    // SAFETY: `GetModuleHandleW(null)` returns this process's HMODULE; `class_name` and
    // `window_title` live across `RegisterClassW` and `CreateWindowExW`. The window uses
    // `WS_EX_TOOLWINDOW | WS_EX_LAYERED` (without `WS_EX_NOACTIVATE` or `WS_EX_TRANSPARENT`)
    // and 0 alpha so it can temporarily become visible/foreground during `TrackPopupMenuEx`
    // without appearing visually or entering Taskbar/Alt+Tab.
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(tray_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        let atom = RegisterClassW(&raw const wc);
        if atom == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(format!(
                "RegisterClassW failed for tray window (err={})",
                GetLastError()
            ));
        }

        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            class_name.as_ptr(),
            window_title.as_ptr(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return Err(format!(
                "CreateWindowExW failed for tray window (err={})",
                GetLastError()
            ));
        }
        let _ = SetLayeredWindowAttributes(hwnd, 0, 0, LWA_ALPHA);
        Ok(hwnd)
    }
}

fn build_notify_icon_data(hwnd: HWND, hicon: HICON, flags: u32) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: u32::try_from(std::mem::size_of::<NOTIFYICONDATAW>()).unwrap_or(0),
        hWnd: hwnd,
        uID: TRAY_ICON_ID,
        uFlags: flags,
        uCallbackMessage: WM_TRAY_CALLBACK,
        hIcon: hicon,
        ..Default::default()
    };
    if (flags & NIF_TIP) != 0 {
        let tip = wide_null("CipherClipboard");
        let max_len = nid.szTip.len().saturating_sub(1);
        let copy_len = tip.len().min(max_len);
        nid.szTip[..copy_len].copy_from_slice(&tip[..copy_len]);
    }
    nid
}

/// # Safety
/// `hwnd` and `hicon` must be live handles owned by this process.
unsafe fn register_notify_icon(hwnd: HWND, hicon: HICON) -> (bool, bool) {
    let nid = build_notify_icon_data(hwnd, hicon, NIF_MESSAGE | NIF_ICON | NIF_TIP);
    if Shell_NotifyIconW(NIM_ADD, &raw const nid) == 0 {
        return (false, false);
    }
    let mut nid_version = build_notify_icon_data(hwnd, std::ptr::null_mut(), 0);
    nid_version.Anonymous = NOTIFYICONDATAW_0 {
        uVersion: NOTIFYICON_VERSION_4,
    };
    let using_v4 = Shell_NotifyIconW(NIM_SETVERSION, &raw const nid_version) != 0;
    (true, using_v4)
}

fn build_tray_popup_menu() -> Result<HMENU, String> {
    // SAFETY: Creates a popup HMENU and populates it with wide null-terminated labels;
    // destroys and frees the HMENU if any `AppendMenuW` call fails.
    unsafe {
        let hmenu = CreatePopupMenu();
        if hmenu.is_null() {
            return Err(format!("CreatePopupMenu failed (err={})", GetLastError()));
        }

        let show_label = wide_null("Show CipherClipboard");
        let settings_label = wide_null("Settings...");
        let quit_label = wide_null("Quit CipherClipboard");

        let populated = AppendMenuW(hmenu, MF_STRING, TRAY_CMD_SHOW_USIZE, show_label.as_ptr())
            != 0
            && AppendMenuW(
                hmenu,
                MF_STRING,
                TRAY_CMD_SETTINGS_USIZE,
                settings_label.as_ptr(),
            ) != 0
            && AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null()) != 0
            && AppendMenuW(hmenu, MF_STRING, TRAY_CMD_QUIT_USIZE, quit_label.as_ptr()) != 0;

        if populated {
            Ok(hmenu)
        } else {
            let err = GetLastError();
            let _ = DestroyMenu(hmenu);
            Err(format!("AppendMenuW failed for tray menu (err={err})"))
        }
    }
}

/// # Safety
/// Must be called on the UI thread owning `hwnd` after `hwnd` is visible (`SWP_SHOWWINDOW`).
unsafe fn acquire_foreground_for_tray_menu(hwnd: HWND) {
    if SetForegroundWindow(hwnd) != 0 {
        return;
    }

    let cur_thread = GetCurrentThreadId();
    let fg_hwnd = GetForegroundWindow();
    let fg_thread = if fg_hwnd.is_null() {
        0
    } else {
        GetWindowThreadProcessId(fg_hwnd, std::ptr::null_mut())
    };
    let attached = fg_thread != 0
        && fg_thread != cur_thread
        && AttachThreadInput(cur_thread, fg_thread, 1) != 0;

    BringWindowToTop(hwnd);
    let activated = SetForegroundWindow(hwnd) != 0;

    if attached {
        let _ = AttachThreadInput(cur_thread, fg_thread, 0);
    }

    if !activated {
        log_message(
            "WARN",
            &format!(
                "SetForegroundWindow failed for tray menu (attached={attached}, err={})",
                GetLastError()
            ),
        );
    }
}

/// # Safety
/// Must run on the UI thread owning `hwnd` synchronously from tray input handling.
unsafe fn show_tray_popup_menu(hwnd: HWND) {
    if TRAY_MENU_OPEN
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let _open_guard = MenuOpenGuard;

    // Clone outside `TrackPopupMenuEx` so the mutex is never held across the modal menu loop.
    let Some((tray_tx, ctx)) = TRAY_STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .map(|state| (state.tray_tx.clone(), state.ctx.clone()))
    else {
        return;
    };

    let mut pt = POINT { x: 0, y: 0 };
    if GetCursorPos(&raw mut pt) == 0 {
        log_message(
            "WARN",
            &format!("GetCursorPos failed for tray menu (err={})", GetLastError()),
        );
        return;
    }

    // Preserve the prior non-CipherClipboard foreground window before claiming focus.
    capture_frontmost();

    let hmenu = match build_tray_popup_menu() {
        Ok(hmenu) => hmenu,
        Err(err) => {
            log_message("WARN", &err);
            return;
        }
    };

    // Place the 0-alpha 1x1 layered owner window at the click point, topmost (above Windows 11
    // taskbar / overflow flyout), and visible so `SetForegroundWindow` and outside clicks work.
    let _ = SetWindowPos(hwnd, HWND_TOPMOST, pt.x, pt.y, 1, 1, SWP_SHOWWINDOW);
    acquire_foreground_for_tray_menu(hwnd);

    let selected = TrackPopupMenuEx(
        hmenu,
        TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
        pt.x,
        pt.y,
        hwnd,
        std::ptr::null(),
    );

    let _ = DestroyMenu(hmenu);
    let _ = ShowWindow(hwnd, SW_HIDE);
    // Microsoft KB135788: force task-switch completion so subsequent tray clicks behave cleanly.
    let _ = PostMessageW(hwnd, WM_NULL, 0, 0);

    let action = match selected {
        TRAY_CMD_SHOW => Some(crate::tray::TrayAction::Show),
        TRAY_CMD_SETTINGS => Some(crate::tray::TrayAction::OpenSettings),
        TRAY_CMD_QUIT => Some(crate::tray::TrayAction::Quit),
        _ => None,
    };
    if let Some(action) = action {
        let _ = tray_tx.send(action);
        ctx.request_repaint();
    }
}

/// # Safety
/// Invoked by Win32 message dispatch for `WindowsTray` helper windows.
unsafe extern "system" fn tray_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let taskbar_msg = TRAY_TASKBAR_CREATED_MSG.load(Ordering::Relaxed);
    if taskbar_msg != 0 && msg == taskbar_msg {
        let hicon = TRAY_STATE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map_or(0, |s| s.hicon);
        if hicon != 0 {
            let del_nid = build_notify_icon_data(hwnd, std::ptr::null_mut(), 0);
            let _ = Shell_NotifyIconW(NIM_DELETE, &raw const del_nid);
            let (live, using_v4) = register_notify_icon(hwnd, hicon as HICON);
            TRAY_ICON_LIVE.store(live, Ordering::Release);
            if let Some(state) = TRAY_STATE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_mut()
            {
                state.using_v4 = using_v4;
            }
        }
        return 0;
    }

    match msg {
        WM_TRAY_CALLBACK => {
            let event = u32::try_from(lparam & 0xFFFF).unwrap_or(0);
            let snapshot = TRAY_STATE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
                .map(|s| (s.using_v4, s.tray_tx.clone(), s.ctx.clone()));
            let Some((using_v4, tray_tx, ctx)) = snapshot else {
                return 0;
            };

            let open_menu =
                (using_v4 && event == WM_CONTEXTMENU) || (!using_v4 && event == WM_RBUTTONUP);
            if open_menu {
                show_tray_popup_menu(hwnd);
                return 0;
            }

            let show_window = (using_v4 && (event == NIN_SELECT || event == NIN_KEYSELECT))
                || (!using_v4 && (event == WM_LBUTTONUP || event == WM_LBUTTONDBLCLK));
            if show_window {
                capture_frontmost();
                let _ = tray_tx.send(crate::tray::TrayAction::Show);
                ctx.request_repaint();
                return 0;
            }
            0
        }
        WM_NCDESTROY => {
            TRAY_ICON_LIVE.store(false, Ordering::Release);
            TRAY_MENU_OPEN.store(false, Ordering::Release);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Native Win32 notification-area icon and context-menu host (`NOTIFYICON_VERSION_4`).
#[derive(Debug)]
pub(crate) struct WindowsTray {
    hwnd: isize,
    hicon: isize,
}

impl WindowsTray {
    pub(crate) fn new(
        rgba: &[u8],
        width: u32,
        height: u32,
        tray_tx: std::sync::mpsc::Sender<crate::tray::TrayAction>,
        ctx: eframe::egui::Context,
    ) -> Result<Self, String> {
        let hicon = create_tray_hicon(rgba, width, height)?;
        let hwnd = match create_tray_window() {
            Ok(hwnd) => hwnd,
            Err(err) => {
                // SAFETY: `hicon` was created above and is unused after failure.
                unsafe {
                    let _ = DestroyIcon(hicon);
                }
                return Err(err);
            }
        };

        let taskbar_created_name = wide_null("TaskbarCreated");
        // SAFETY: `taskbar_created_name` is null-terminated; `hwnd` is a valid window owned by this thread.
        unsafe {
            let taskbar_msg = RegisterWindowMessageW(taskbar_created_name.as_ptr());
            if taskbar_msg != 0 {
                TRAY_TASKBAR_CREATED_MSG.store(taskbar_msg, Ordering::Relaxed);
                let _ = ChangeWindowMessageFilterEx(
                    hwnd,
                    taskbar_msg,
                    MSGFLT_ALLOW,
                    std::ptr::null_mut(),
                );
            }
        }

        let mut registered = false;
        let mut using_v4 = false;
        for attempt in 0..5 {
            // SAFETY: `hwnd` and `hicon` are valid handles created above.
            let (ok, v4) = unsafe { register_notify_icon(hwnd, hicon) };
            if ok {
                registered = true;
                using_v4 = v4;
                break;
            }
            if attempt < 4 {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }

        if !registered {
            // SAFETY: Clean up `hwnd` and `hicon` before returning initialization failure.
            unsafe {
                let _ = DestroyWindow(hwnd);
                let _ = DestroyIcon(hicon);
            }
            return Err("Shell_NotifyIconW(NIM_ADD) failed (Windows shell not ready)".to_string());
        }

        TRAY_ICON_LIVE.store(true, Ordering::Release);
        *TRAY_STATE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(TrayState {
            tray_tx,
            ctx,
            hicon: hicon as isize,
            using_v4,
        });

        Ok(Self {
            hwnd: hwnd as isize,
            hicon: hicon as isize,
        })
    }

    pub(crate) fn is_live(&self) -> bool {
        if self.hwnd == 0 || !TRAY_ICON_LIVE.load(Ordering::Acquire) {
            return false;
        }
        // SAFETY: `IsWindow` safely validates whether `self.hwnd` is still an active HWND.
        unsafe { IsWindow(self.hwnd as HWND) != 0 }
    }
}

impl Drop for WindowsTray {
    fn drop(&mut self) {
        TRAY_ICON_LIVE.store(false, Ordering::Release);
        *TRAY_STATE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;

        if self.hwnd != 0 {
            // SAFETY: Removes the shell notification icon and destroys the owned helper HWND.
            unsafe {
                let hwnd = self.hwnd as HWND;
                let nid = build_notify_icon_data(hwnd, std::ptr::null_mut(), 0);
                let _ = Shell_NotifyIconW(NIM_DELETE, &raw const nid);
                if IsWindow(hwnd) != 0 {
                    let _ = DestroyWindow(hwnd);
                }
            }
            self.hwnd = 0;
        }
        if self.hicon != 0 {
            // SAFETY: Frees the `HICON` created by `CreateIcon` in `WindowsTray::new`.
            unsafe {
                let _ = DestroyIcon(self.hicon as HICON);
            }
            self.hicon = 0;
        }
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

static REPAINT_CTX: Mutex<Option<eframe::egui::Context>> = Mutex::new(None);
static REOPEN_REQUESTED: AtomicBool = AtomicBool::new(false);
static REOPEN_SHUTDOWN: Mutex<Option<usize>> = Mutex::new(None);
static REOPEN_THREAD: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

pub(super) fn set_repaint_context(ctx: &eframe::egui::Context) {
    if let Ok(mut lock) = REPAINT_CTX.lock() {
        *lock = Some(ctx.clone());
    }
}

fn request_repaint() {
    if let Ok(lock) = REPAINT_CTX.lock() {
        if let Some(ctx) = lock.as_ref() {
            ctx.request_repaint();
        }
    }
}

pub(super) fn take_reopen_requested() -> bool {
    REOPEN_REQUESTED.swap(false, Ordering::AcqRel)
}

fn reopen_event_name(pid: u32) -> String {
    format!("Local\\CipherClipboard_Reopen_{pid}")
}

pub(super) fn setup_reopen_observer() {
    teardown_reopen_observer();

    // SAFETY: GetCurrentProcessId is side-effect-free.
    let pid = unsafe { GetCurrentProcessId() };
    let event_name = reopen_event_name(pid);
    let name_wide: Vec<u16> = OsStr::new(&event_name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: name_wide is null-terminated and live for the call.
    let reopen_event = unsafe { CreateEventW(std::ptr::null(), 0, 0, name_wide.as_ptr()) };
    if reopen_event.is_null() {
        log_message("WARN", "Failed to create Windows reopen event");
        return;
    }

    // SAFETY: Anonymous manual-reset event with null security attributes.
    let shutdown_event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if shutdown_event.is_null() {
        // SAFETY: reopen_event was checked non-null above.
        unsafe {
            CloseHandle(reopen_event);
        }
        log_message("WARN", "Failed to create Windows reopen shutdown event");
        return;
    }

    if let Ok(mut lock) = REOPEN_SHUTDOWN.lock() {
        *lock = Some(shutdown_event as usize);
    }

    let reopen_raw = reopen_event as usize;
    let shutdown_raw = shutdown_event as usize;

    let handle = std::thread::Builder::new()
        .name("clipboard-windows-reopen".to_string())
        .spawn(move || {
            let handles = [shutdown_raw as HANDLE, reopen_raw as HANDLE];
            loop {
                // SAFETY: handles points to two valid event handles; 0 (FALSE) waits for any object.
                let wait_res = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
                if wait_res == WAIT_OBJECT_0 {
                    break;
                } else if wait_res == WAIT_OBJECT_0 + 1 {
                    REOPEN_REQUESTED.store(true, Ordering::SeqCst);
                    request_repaint();
                } else {
                    break;
                }
            }
            // SAFETY: reopen_raw was created in setup_reopen_observer and is owned by this background thread.
            unsafe {
                CloseHandle(reopen_raw as HANDLE);
            }
        });

    if let Ok(handle) = handle {
        if let Ok(mut lock) = REOPEN_THREAD.lock() {
            *lock = Some(handle);
        }
    } else {
        // SAFETY: Both handles were checked non-null above.
        unsafe {
            CloseHandle(reopen_event);
            CloseHandle(shutdown_event);
        }
    }
}

pub(super) fn teardown_reopen_observer() {
    let shutdown_handle = REOPEN_SHUTDOWN.lock().ok().and_then(|mut lock| lock.take());
    if let Some(raw) = shutdown_handle {
        let handle = raw as HANDLE;
        // SAFETY: handle is a live manual-reset event.
        unsafe {
            SetEvent(handle);
        }
    }

    let thread = REOPEN_THREAD.lock().ok().and_then(|mut lock| lock.take());
    if let Some(handle) = thread {
        let _ = handle.join();
    }

    if let Some(raw) = shutdown_handle {
        let handle = raw as HANDLE;
        // SAFETY: handle is closed once after joining the background thread.
        unsafe {
            CloseHandle(handle);
        }
    }
}

fn signal_reopen_event(pid: u32) -> bool {
    let event_name = reopen_event_name(pid);
    let name_wide: Vec<u16> = OsStr::new(&event_name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: name_wide is null-terminated and live for the call.
    let h_event = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name_wide.as_ptr()) };
    if h_event.is_null() {
        false
    } else {
        // SAFETY: h_event is non-null and closed immediately after signaling.
        let ok = unsafe { SetEvent(h_event) };
        unsafe {
            CloseHandle(h_event);
        }
        ok != 0
    }
}

struct InstanceSearchCtx {
    expected_pid: Option<u32>,
    found: HWND,
    found_pid: u32,
}

// SAFETY: `EnumWindows` invokes this callback synchronously on the calling thread, so `lparam`
// is non-null, correctly aligned, and uniquely borrows the caller's live stack-local
// `InstanceSearchCtx` for the duration of the call; no callback state is retained or shared
// across threads. The title buffer is indexed only by the length `GetWindowTextW` reports.
unsafe extern "system" fn enum_instance_window_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam as *mut InstanceSearchCtx);
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
                ctx.found_pid = pid;
                return 0;
            }
        }
    }
    1
}

pub(crate) fn activate_existing_instance(expected_pid: Option<u32>) -> bool {
    let signaled = expected_pid.is_some_and(signal_reopen_event);

    let mut ctx = InstanceSearchCtx {
        expected_pid,
        found: std::ptr::null_mut(),
        found_pid: 0,
    };
    // SAFETY: `enum_instance_window_proc` is a valid callback whose `lparam` uniquely borrows
    // this stack-local `ctx` for the synchronous `EnumWindows` call; `addr_of_mut!` forms the raw
    // pointer without creating an aliasing reference. Stale HWNDs fail safely in User32 calls.
    unsafe {
        EnumWindows(
            Some(enum_instance_window_proc),
            std::ptr::addr_of_mut!(ctx) as LPARAM,
        );
        if ctx.found.is_null() {
            signaled
        } else {
            if !signaled && ctx.found_pid != 0 {
                let _ = signal_reopen_event(ctx.found_pid);
            }
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

// TOKEN_MANDATORY_LABEL = SID_AND_ATTRIBUTES followed inline by the SID bytes.
// Allocate a properly aligned 256-byte buffer so GetTokenInformation can store
// both the header and the inline integrity SID without ERROR_INSUFFICIENT_BUFFER.
#[repr(C, align(8))]
struct TokenIntegrityBuf([u8; 256]);

/// Extracts the integrity RID from a validated mandatory-label SID.
///
/// # Safety
/// `label` must be an initialized `TOKEN_MANDATORY_LABEL` returned by
/// `GetTokenInformation`; its SID is validated again here before dereference.
unsafe fn integrity_rid(label: &TOKEN_MANDATORY_LABEL) -> Option<u32> {
    if label.Label.Sid.is_null() || IsValidSid(label.Label.Sid) == 0 {
        return None;
    }
    let count = GetSidSubAuthorityCount(label.Label.Sid);
    if count.is_null() || *count == 0 {
        return None;
    }
    let rid = GetSidSubAuthority(label.Label.Sid, u32::from(*count - 1));
    if rid.is_null() {
        return None;
    }
    Some(*rid)
}

/// Reads the mandatory integrity level of `pid`, or `None` if it cannot be queried.
///
/// # Safety
/// All Win32 handle calls are null-checked and closed; `ret_len` is validated
/// to be at least the `TOKEN_MANDATORY_LABEL` header and no larger than the
/// 8-byte-aligned 256-byte buffer before the cast.
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
            level = integrity_rid(info);
        }
        let _ = CloseHandle(token);
    }
    let _ = CloseHandle(proc);
    level
}

/// True when the captured target runs at a higher mandatory integrity level
/// than us (normally: it runs as Administrator). UIPI silently discards the
/// injected Ctrl+V across that boundary, so auto-paste must be skipped.
pub(crate) fn target_is_elevated() -> bool {
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
/// Returns an [`std::io::Error`] if the input is too large for DPAPI or `CryptProtectData` fails.
pub(crate) fn dpapi_protect(plaintext: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let in_len = u32::try_from(plaintext.len())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let entropy_len = u32::try_from(DPAPI_ENTROPY.len())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

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
        return Err(std::io::Error::last_os_error());
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
/// Returns an [`std::io::Error`] if the input is too large for DPAPI, `CryptUnprotectData`
/// fails, or the ciphertext was not produced with the application entropy.
pub(crate) fn dpapi_unprotect(
    ciphertext: &[u8],
) -> Result<zeroize::Zeroizing<Vec<u8>>, std::io::Error> {
    use zeroize::Zeroize;

    let in_len = u32::try_from(ciphertext.len())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let entropy_len = u32::try_from(DPAPI_ENTROPY.len())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

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
        return Err(std::io::Error::last_os_error());
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

static OCR_SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub(super) fn supports_image_ocr() -> bool {
    *OCR_SUPPORTED.get_or_init(|| OcrEngine::TryCreateFromUserProfileLanguages().is_ok())
}

pub(super) fn extract_text_from_png(png_bytes: &[u8]) -> Result<String, String> {
    if png_bytes.is_empty() {
        return Err("Image payload is empty".to_string());
    }

    let (width, height, mut bgra) =
        crate::media::png_to_rgba(png_bytes).map_err(|e| format!("Failed to decode PNG: {e}"))?;

    let (chunks, _) = bgra.as_chunks_mut::<4>();
    for pixel in chunks {
        pixel.swap(0, 2);
    }

    let width_i32 =
        i32::try_from(width).map_err(|_| "Image width exceeds integer limit".to_string())?;
    let height_i32 =
        i32::try_from(height).map_err(|_| "Image height exceeds integer limit".to_string())?;

    let writer =
        DataWriter::new().map_err(|e| format!("Failed to create Windows DataWriter: {e}"))?;
    writer
        .WriteBytes(&bgra)
        .map_err(|e| format!("Failed to write image buffer: {e}"))?;
    let buffer = writer
        .DetachBuffer()
        .map_err(|e| format!("Failed to detach image buffer: {e}"))?;

    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        width_i32,
        height_i32,
        BitmapAlphaMode::Premultiplied,
    )
    .map_err(|e| format!("Failed to create SoftwareBitmap: {e}"))?;

    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|e| format!("No OCR language pack installed for user profile: {e}"))?;

    let op = engine
        .RecognizeAsync(&bitmap)
        .map_err(|e| format!("Failed to start OCR operation: {e}"))?;

    let result = op
        .join()
        .map_err(|e| format!("OCR recognition failed: {e}"))?;

    let text = result
        .Text()
        .map_err(|e| format!("Failed to retrieve OCR text: {e}"))?
        .to_string();

    Ok(text.replace("\r\n", "\n"))
}

#[cfg(test)]
mod tests {
    use super::{
        build_tray_popup_menu, create_tray_hicon, create_tray_window, DestroyIcon, DestroyMenu,
        DestroyWindow, IsWindow, WindowsTray, TRAY_ICON_LIVE, TRAY_STATE,
    };
    use std::sync::atomic::Ordering;

    #[test]
    fn test_windows_tray_primitives_and_cleanup() {
        let invalid = create_tray_hicon(&[0u8; 4], 16, 16);
        assert!(invalid.is_err());

        let rgba = vec![255u8; 16 * 16 * 4];
        let hicon = create_tray_hicon(&rgba, 16, 16).expect("32bpp tray icon should build");
        assert!(!hicon.is_null());

        let hwnd = create_tray_window().expect("tray helper HWND should build");
        assert!(!hwnd.is_null());
        // SAFETY: `hwnd` was created by `create_tray_window` and is validated before destruction.
        unsafe {
            assert_ne!(IsWindow(hwnd), 0);
        }

        let hmenu = build_tray_popup_menu().expect("tray popup HMENU should build");
        assert!(!hmenu.is_null());

        // SAFETY: Cleans up the Win32 resources created in this test.
        unsafe {
            assert_ne!(DestroyMenu(hmenu), 0);
            assert_ne!(DestroyWindow(hwnd), 0);
            assert_eq!(IsWindow(hwnd), 0);
            assert_ne!(DestroyIcon(hicon), 0);
        }

        // On headless CI runners Explorer may not host a notification area, so `Shell_NotifyIconW`
        // can cleanly return an error; if it succeeds, dropping `WindowsTray` must clean up.
        let (tx, _rx) = std::sync::mpsc::channel();
        let ctx = eframe::egui::Context::default();
        if let Ok(tray) = WindowsTray::new(&rgba, 16, 16, tx, ctx) {
            assert!(tray.is_live());
            drop(tray);
            assert!(!TRAY_ICON_LIVE.load(Ordering::Acquire));
            assert!(TRAY_STATE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_none());
        }
    }
}
