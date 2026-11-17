#![allow(unsafe_code)]
#![allow(unexpected_cfgs)]

//! Cross-platform focus restoration and simulated paste automation.
//!
//! When `CipherClipboard` is summoned via a global hotkey, it takes OS window focus.
//! This module captures the active application/window *before* `CipherClipboard` takes
//! focus, and restores focus and simulates `Cmd+V` (macOS) or `Ctrl+V` (Windows/Linux)
//! when an item is chosen.

use std::time::Duration;

#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

#[cfg(target_os = "macos")]
static PREVIOUS_APP_PID: AtomicI32 = AtomicI32::new(0);

#[cfg(target_os = "macos")]
static REPAINT_CTX: std::sync::Mutex<Option<eframe::egui::Context>> = std::sync::Mutex::new(None);

/// Stores a clone of the `egui::Context` to immediately trigger repaints on OS activation.
#[allow(dead_code, clippy::missing_const_for_fn)]
pub(crate) fn set_repaint_context(ctx: &eframe::egui::Context) {
    #[cfg(target_os = "macos")]
    if let Ok(mut lock) = REPAINT_CTX.lock() {
        *lock = Some(ctx.clone());
    }
    #[cfg(not(target_os = "macos"))]
    let _ = ctx;
}

#[cfg(target_os = "macos")]
pub(super) fn request_repaint() {
    if let Ok(lock) = REPAINT_CTX.lock() {
        if let Some(ctx) = lock.as_ref() {
            ctx.request_repaint();
        }
    }
}

/// Returns true if the application currently has OS focus/activation.
#[allow(dead_code, clippy::missing_const_for_fn)]
#[must_use]
pub(crate) fn is_app_active() -> bool {
    is_app_active_state().unwrap_or(false)
}

/// Returns `Some(true)` if this application is currently active at the OS/WM level,
/// `Some(false)` if another application is active, or `None` if the platform/session
/// cannot determine active-window ownership (e.g. pure Wayland without X11).
#[allow(clippy::missing_const_for_fn)]
#[must_use]
pub(crate) fn is_app_active_state() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        Some(macos::is_active())
    }
    #[cfg(target_os = "windows")]
    {
        Some(windows::is_active())
    }
    #[cfg(target_os = "linux")]
    {
        linux::is_active()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        None
    }
}

/// Returns `true` on Windows when the current keyboard layout produces `ù` for the
/// physical key in the US `Quote` position (French/Belgian AZERTY layouts).
#[allow(dead_code, clippy::missing_const_for_fn)]
#[must_use]
pub(crate) fn windows_quote_key_is_ugrave() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::quote_key_is_ugrave()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Checks if the current clipboard content is marked as concealed or transient
/// by password managers (e.g. 1Password, Bitwarden, `KeePass`, macOS Keychain).
#[allow(clippy::missing_const_for_fn)]
#[must_use]
pub(crate) fn is_clipboard_concealed() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::is_clipboard_concealed()
    }
    #[cfg(target_os = "windows")]
    {
        windows::is_clipboard_concealed()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

/// Hides the entire application at the OS level (macOS `[NSApp hide:]`).
#[allow(dead_code, clippy::missing_const_for_fn)]
pub(crate) fn hide_application() {
    #[cfg(target_os = "macos")]
    macos::hide_app();
}

/// Unhides and orders the main window front at the OS level.
#[allow(clippy::missing_const_for_fn)]
pub(crate) fn order_front_main_window() {
    #[cfg(target_os = "macos")]
    macos::order_front_main_window();

    #[cfg(target_os = "windows")]
    windows::order_front_main_window();
}

/// On Windows, activates and brings to front any existing running instance window.
#[cfg(target_os = "windows")]
pub(crate) fn activate_existing_instance() -> bool {
    windows::activate_existing_instance()
}

/// Displays a native modal error dialog on Windows before terminating.
#[cfg(target_os = "windows")]
pub(crate) fn show_error_dialog(message: &str) {
    windows::show_error_dialog(message);
}

/// Checks and clears any pending reopen requests triggered by OS events (e.g. Dock icon click).
#[allow(clippy::missing_const_for_fn)]
#[must_use]
pub(crate) fn take_reopen_requested() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::take_reopen_requested()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Registers OS-level activation and reopen observers (e.g. macOS Dock click / Cmd+Tab notifications).
#[allow(clippy::missing_const_for_fn)]
pub(crate) fn setup_reopen_observer() {
    #[cfg(target_os = "macos")]
    macos::setup_reopen_observer();
}

/// Removes the macOS reopen observer to prevent crashes on app termination.
#[allow(clippy::missing_const_for_fn)]
pub(crate) fn teardown_reopen_observer() {
    #[cfg(target_os = "macos")]
    macos::teardown_reopen_observer();
}

/// Captures the current frontmost application/window before `CipherClipboard` gains focus.
#[allow(clippy::missing_const_for_fn)]
pub(crate) fn capture_active_window() {
    #[cfg(target_os = "macos")]
    macos::capture_frontmost();

    #[cfg(target_os = "windows")]
    windows::capture_frontmost();

    #[cfg(target_os = "linux")]
    linux::capture_frontmost();
}

/// Restores focus to the previous application and optionally simulates a paste keystroke.
pub(crate) fn restore_focus_and_paste(
    restore_focus: bool,
    auto_paste: bool,
    auto_paste_delay_ms: u64,
) {
    if !restore_focus && !auto_paste {
        return;
    }

    std::thread::spawn(move || {
        // If auto_paste is active, focus MUST be switched to the target application
        // so the simulated keystrokes are received by that application.
        let should_restore = restore_focus || auto_paste;

        if should_restore {
            #[cfg(target_os = "macos")]
            macos::restore_focus();

            #[cfg(target_os = "windows")]
            windows::restore_focus();

            #[cfg(target_os = "linux")]
            linux::restore_focus();
        }

        if auto_paste {
            // Give the OS window server time to switch key focus
            // to the target application before dispatching keystrokes.
            std::thread::sleep(Duration::from_millis(auto_paste_delay_ms));

            #[cfg(target_os = "macos")]
            macos::simulate_paste();

            #[cfg(target_os = "windows")]
            windows::simulate_paste();

            #[cfg(target_os = "linux")]
            linux::simulate_paste();
        }
    });
}

/// Checks whether macOS Accessibility permissions are granted for synthetic event posting.
#[allow(dead_code, clippy::missing_const_for_fn)]
#[must_use]
pub(crate) fn is_accessibility_trusted() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::is_trusted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Prompts the user for macOS Accessibility permissions using the native system dialog.
#[allow(dead_code, clippy::missing_const_for_fn)]
pub(crate) fn prompt_accessibility_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::prompt_permission()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Opens macOS System Settings directly to Privacy & Security > Accessibility.
#[allow(dead_code, clippy::missing_const_for_fn)]
pub(crate) fn open_accessibility_settings() {
    #[cfg(target_os = "macos")]
    macos::open_accessibility_settings();
}

// ---------------------------------------------------------------------------
// macOS Implementation
// ---------------------------------------------------------------------------
#[cfg(target_os = "macos")]
mod macos {
    use super::{AtomicBool, Ordering, PREVIOUS_APP_PID};
    use objc::declare::ClassDecl;
    use objc::runtime::{Object, Sel};
    use objc::{class, msg_send, sel, sel_impl};

    static REOPEN_REQUESTED: AtomicBool = AtomicBool::new(false);

    pub(super) fn take_reopen_requested() -> bool {
        REOPEN_REQUESTED.swap(false, Ordering::SeqCst)
    }

    pub(super) fn is_active() -> bool {
        unsafe {
            let ns_app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            if ns_app.is_null() {
                return false;
            }
            let active: bool = msg_send![ns_app, isActive];
            let hidden: bool = msg_send![ns_app, isHidden];
            active && !hidden
        }
    }

    pub(super) fn hide_app() {
        unsafe {
            let ns_app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            if !ns_app.is_null() {
                let nil_obj: *mut Object = std::ptr::null_mut();
                let _: () = msg_send![ns_app, hide: nil_obj];
            }
        }
    }

    pub(super) fn order_front_main_window() {
        unsafe {
            let ns_app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            if ns_app.is_null() {
                return;
            }
            let nil_obj: *mut Object = std::ptr::null_mut();
            let _: () = msg_send![ns_app, unhide: nil_obj];
            let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];

            let windows: *mut Object = msg_send![ns_app, windows];
            if !windows.is_null() {
                let count: usize = msg_send![windows, count];
                for i in 0..count {
                    let window: *mut Object = msg_send![windows, objectAtIndex: i];
                    if !window.is_null() {
                        let _: () = msg_send![window, setIsVisible: true];
                        let _: () = msg_send![window, makeKeyAndOrderFront: nil_obj];
                        let _: () = msg_send![window, orderFrontRegardless];
                    }
                }
            }
        }
    }

    extern "C" fn on_app_reopen(_this: &Object, _cmd: Sel, _notification: *mut Object) {
        REOPEN_REQUESTED.store(true, Ordering::SeqCst);
        order_front_main_window();
        super::request_repaint();
    }

    // Wrapper to allow storing raw pointer in static (only accessed from main thread)
    struct ObserverPtr(*mut Object);
    unsafe impl Send for ObserverPtr {}
    unsafe impl Sync for ObserverPtr {}

    static REOPEN_OBSERVER: std::sync::Mutex<Option<ObserverPtr>> = std::sync::Mutex::new(None);

    pub(super) fn setup_reopen_observer() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| unsafe {
            let superclass = class!(NSObject);
            if let Some(mut decl) = ClassDecl::new("CipherReopenObserver", superclass) {
                decl.add_method(
                    sel!(onNotification:),
                    on_app_reopen as extern "C" fn(&Object, Sel, *mut Object),
                );
                decl.register();
            }

            let observer_cls = class!(CipherReopenObserver);
            let observer: *mut Object = msg_send![observer_cls, new];
            let center: *mut Object = msg_send![class!(NSNotificationCenter), defaultCenter];
            if center.is_null() || observer.is_null() {
                return;
            }

            let nil_obj: *mut Object = std::ptr::null_mut();

            // Observe NSApplicationDidBecomeActiveNotification (fires on Dock click, Cmd+Tab, app activate)
            let active_name: *mut Object = msg_send![
                class!(NSString),
                stringWithUTF8String: b"NSApplicationDidBecomeActiveNotification\0".as_ptr()
            ];
            if !active_name.is_null() {
                let _: () = msg_send![
                    center,
                    addObserver: observer
                    selector: sel!(onNotification:)
                    name: active_name
                    object: nil_obj
                ];
            }

            // Observe NSApplicationDidUnhideNotification (fires when macOS unhides on Dock click / Cmd+Tab)
            let unhide_name: *mut Object = msg_send![
                class!(NSString),
                stringWithUTF8String: b"NSApplicationDidUnhideNotification\0".as_ptr()
            ];
            if !unhide_name.is_null() {
                let _: () = msg_send![
                    center,
                    addObserver: observer
                    selector: sel!(onNotification:)
                    name: unhide_name
                    object: nil_obj
                ];
            }

            // Store observer for later cleanup
            if let Ok(mut lock) = REOPEN_OBSERVER.lock() {
                *lock = Some(ObserverPtr(observer));
            }
        });
    }

    pub(super) fn teardown_reopen_observer() {
        let observer = if let Ok(mut lock) = REOPEN_OBSERVER.lock() {
            lock.take()
        } else {
            None
        };
        if let Some(ObserverPtr(observer)) = observer {
            unsafe {
                let center: *mut Object = msg_send![class!(NSNotificationCenter), defaultCenter];
                if !center.is_null() {
                    let _: () = msg_send![center, removeObserver: observer];
                }
            }
        }
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> bool;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceCreate(state_id: i32) -> *mut std::ffi::c_void;
        fn CGEventCreateKeyboardEvent(
            source: *mut std::ffi::c_void,
            virtual_key: u16,
            key_down: bool,
        ) -> *mut std::ffi::c_void;
        fn CGEventSetFlags(event: *mut std::ffi::c_void, flags: u64);
        fn CGEventPost(tap: u32, event: *mut std::ffi::c_void);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const std::ffi::c_void);
    }

    pub(super) fn is_trusted() -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    pub(super) fn prompt_permission() -> bool {
        unsafe {
            let key = b"AXTrustedCheckOptionPrompt\0";
            let str_obj: *mut Object =
                msg_send![class!(NSString), stringWithUTF8String: key.as_ptr()];
            let num_obj: *mut Object = msg_send![class!(NSNumber), numberWithBool: true];
            let dict: *mut Object =
                msg_send![class!(NSDictionary), dictionaryWithObject: num_obj forKey: str_obj];
            AXIsProcessTrustedWithOptions(dict as *const std::ffi::c_void)
        }
    }

    pub(super) fn open_accessibility_settings() {
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn();
    }

    pub(super) fn capture_frontmost() {
        unsafe {
            let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
            if workspace.is_null() {
                return;
            }
            let frontmost: *mut Object = msg_send![workspace, frontmostApplication];
            if frontmost.is_null() {
                return;
            }
            let pid: i32 = msg_send![frontmost, processIdentifier];
            let our_pid = std::process::id() as i32;

            if pid > 0 && pid != our_pid {
                PREVIOUS_APP_PID.store(pid, Ordering::SeqCst);
            }
        }
    }

    pub(super) fn restore_focus() {
        unsafe {
            // 1. Hide CipherClipboard application so macOS automatically yields focus
            let ns_app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            if !ns_app.is_null() {
                let nil_obj: *mut Object = std::ptr::null_mut();
                let _: () = msg_send![ns_app, hide: nil_obj];
            }

            // 2. Explicitly reactivate the target process by its PID
            let pid = PREVIOUS_APP_PID.load(Ordering::SeqCst);
            if pid > 0 {
                let target_app: *mut Object = msg_send![
                    class!(NSRunningApplication),
                    runningApplicationWithProcessIdentifier: pid
                ];
                if !target_app.is_null() {
                    // NSApplicationActivateIgnoringOtherApps = 1 << 1 (2)
                    const ACTIVATE_IGNORING_OTHER_APPS: usize = 2;
                    let _: bool = msg_send![
                        target_app,
                        activateWithOptions: ACTIVATE_IGNORING_OTHER_APPS
                    ];
                }
            }
        }
    }

    pub(super) fn simulate_paste() {
        // macOS ANSI virtual keycode for 'v' is 9
        const KEY_V: u16 = 9;
        // kCGEventFlagMaskCommand = 0x0010_0000
        const MASK_COMMAND: u64 = 0x0010_0000;
        // kCGHIDEventTap = 0, kCGSessionEventTap = 1
        const TAP_LOCATION_HID: u32 = 0;
        const TAP_LOCATION_SESSION: u32 = 1;
        // kCGEventSourceStateCombinedSessionState = 0
        const SOURCE_STATE_COMBINED_SESSION: i32 = 0;

        unsafe {
            let source = CGEventSourceCreate(SOURCE_STATE_COMBINED_SESSION);

            // Press Command+V
            let event_down = CGEventCreateKeyboardEvent(source, KEY_V, true);
            if !event_down.is_null() {
                CGEventSetFlags(event_down, MASK_COMMAND);
                CGEventPost(TAP_LOCATION_HID, event_down);
                CGEventPost(TAP_LOCATION_SESSION, event_down);
                CFRelease(event_down);
            }

            // Brief delay between key down and key up to ensure runloop registration
            std::thread::sleep(std::time::Duration::from_millis(15));

            // Release Command+V
            let event_up = CGEventCreateKeyboardEvent(source, KEY_V, false);
            if !event_up.is_null() {
                CGEventSetFlags(event_up, MASK_COMMAND);
                CGEventPost(TAP_LOCATION_HID, event_up);
                CGEventPost(TAP_LOCATION_SESSION, event_up);
                CFRelease(event_up);
            }

            if !source.is_null() {
                CFRelease(source);
            }
        }
    }

    pub(super) fn is_clipboard_concealed() -> bool {
        unsafe {
            let pasteboard: *mut Object = msg_send![class!(NSPasteboard), generalPasteboard];
            if pasteboard.is_null() {
                return false;
            }
            let types: *mut Object = msg_send![pasteboard, types];
            if types.is_null() {
                return false;
            }
            let count: usize = msg_send![types, count];

            for i in 0..count {
                let item_type: *mut Object = msg_send![types, objectAtIndex: i];
                if item_type.is_null() {
                    continue;
                }
                let utf8_ptr: *const std::ffi::c_char = msg_send![item_type, UTF8String];
                if !utf8_ptr.is_null() {
                    if let Ok(type_str) = std::ffi::CStr::from_ptr(utf8_ptr).to_str() {
                        for marker in &[
                            "org.nspasteboard.ConcealedType",
                            "org.nspasteboard.AutoGeneratedType",
                            "org.nspasteboard.TransientType",
                            "com.agilebits.onepassword",
                            "de.petermaurer.TransientPasteboardType",
                        ] {
                            if type_str == *marker {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Windows Implementation
// ---------------------------------------------------------------------------
#[cfg(target_os = "windows")]
mod windows {
    use std::sync::atomic::{AtomicIsize, Ordering};

    static PREVIOUS_HWND: AtomicIsize = AtomicIsize::new(0);

    type HWND = *mut std::ffi::c_void;

    extern "system" {
        fn GetForegroundWindow() -> HWND;
        fn SetForegroundWindow(hwnd: HWND) -> i32;
        fn BringWindowToTop(hwnd: HWND) -> i32;
        fn ShowWindow(hwnd: HWND, nCmdShow: i32) -> i32;
        fn IsIconic(hwnd: HWND) -> i32;
        fn GetCurrentProcessId() -> u32;
        fn GetCurrentThreadId() -> u32;
        fn GetWindowThreadProcessId(hwnd: HWND, lpdwProcessId: *mut u32) -> u32;
        fn AttachThreadInput(idAttach: u32, idAttachTo: u32, fAttach: i32) -> i32;
        fn EnumWindows(
            lpEnumFunc: unsafe extern "system" fn(HWND, isize) -> i32,
            lParam: isize,
        ) -> i32;
        fn GetWindowTextW(hwnd: HWND, lpString: *mut u16, nMaxCount: i32) -> i32;
    }

    pub(super) fn order_front_main_window() {
        unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: isize) -> i32 {
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == GetCurrentProcessId() {
                let mut buf = [0u16; 64];
                let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
                if len > 0 {
                    let title = String::from_utf16_lossy(&buf[..len as usize]);
                    if title == "CipherClipboard" {
                        let out = lparam as *mut HWND;
                        if !out.is_null() {
                            *out = hwnd;
                        }
                        return 0; // stop enumeration
                    }
                }
            }
            1
        }

        const SW_SHOW: i32 = 5;
        const SW_RESTORE: i32 = 9;

        unsafe {
            let mut target_hwnd: HWND = std::ptr::null_mut();
            EnumWindows(enum_proc, (&mut target_hwnd as *mut HWND) as isize);
            if target_hwnd.is_null() {
                return;
            }

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
                GetWindowThreadProcessId(fg_hwnd, &mut fg_pid)
            };

            if fg_thread != 0 && fg_thread != cur_thread {
                AttachThreadInput(cur_thread, fg_thread, 1);
                BringWindowToTop(target_hwnd);
                SetForegroundWindow(target_hwnd);
                AttachThreadInput(cur_thread, fg_thread, 0);
            } else {
                BringWindowToTop(target_hwnd);
                SetForegroundWindow(target_hwnd);
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
            GetWindowThreadProcessId(hwnd, &mut pid);
            pid == GetCurrentProcessId()
        }
    }

    pub(super) fn capture_frontmost() {
        unsafe {
            let hwnd = GetForegroundWindow();
            if !hwnd.is_null() {
                let mut pid: u32 = 0;
                GetWindowThreadProcessId(hwnd, &mut pid);
                if pid != GetCurrentProcessId() {
                    PREVIOUS_HWND.store(hwnd as isize, Ordering::SeqCst);
                }
            }
        }
    }

    pub(super) fn restore_focus() {
        let hwnd_raw = PREVIOUS_HWND.load(Ordering::SeqCst);
        if hwnd_raw != 0 {
            unsafe {
                let target_hwnd = hwnd_raw as HWND;
                let cur_thread = GetCurrentThreadId();
                let mut target_pid: u32 = 0;
                let target_thread = GetWindowThreadProcessId(target_hwnd, &mut target_pid);

                if target_thread != 0 && target_thread != cur_thread {
                    AttachThreadInput(cur_thread, target_thread, 1);
                    BringWindowToTop(target_hwnd);
                    SetForegroundWindow(target_hwnd);
                    AttachThreadInput(cur_thread, target_thread, 0);
                } else {
                    BringWindowToTop(target_hwnd);
                    SetForegroundWindow(target_hwnd);
                }
            }
        }
    }

    pub(super) fn activate_existing_instance() -> bool {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        let title_wide: Vec<u16> = OsStr::new("CipherClipboard")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        extern "system" {
            fn FindWindowW(lpClassName: *const u16, lpWindowName: *const u16) -> HWND;
            fn ShowWindow(hwnd: HWND, nCmdShow: i32) -> i32;
        }

        const SW_RESTORE: i32 = 9;
        const SW_SHOW: i32 = 5;

        unsafe {
            let hwnd = FindWindowW(std::ptr::null(), title_wide.as_ptr());
            if !hwnd.is_null() {
                ShowWindow(hwnd, SW_RESTORE);
                ShowWindow(hwnd, SW_SHOW);
                BringWindowToTop(hwnd);
                SetForegroundWindow(hwnd);
                true
            } else {
                false
            }
        }
    }

    pub(super) fn show_error_dialog(message: &str) {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        let msg_wide: Vec<u16> = OsStr::new(message)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let title_wide: Vec<u16> = OsStr::new("CipherClipboard Error")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        extern "system" {
            fn MessageBoxW(
                hwnd: HWND,
                lpText: *const u16,
                lpCaption: *const u16,
                uType: u32,
            ) -> i32;
        }

        const MB_OK: u32 = 0x0000_0000;
        const MB_ICONERROR: u32 = 0x0000_0010;

        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                msg_wide.as_ptr(),
                title_wide.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }

    pub(super) fn simulate_paste() {
        #[allow(non_snake_case, non_camel_case_types)]
        #[repr(C)]
        struct KEYBDINPUT {
            wVk: u16,
            wScan: u16,
            dwFlags: u32,
            time: u32,
            dwExtraInfo: usize,
        }

        // Win32 INPUT union is sized to MOUSEINPUT (32 bytes on x64, 24 bytes on x86),
        // making sizeof(INPUT) == 40 bytes on x64 (4 + 4 align + 24 KEYBDINPUT + 8 pad)
        // and 28 bytes on x86 (4 + 16 KEYBDINPUT + 8 pad).
        #[allow(non_snake_case, non_camel_case_types)]
        #[repr(C)]
        struct INPUT {
            r#type: u32,
            u: KEYBDINPUT,
            _pad: [u8; 8],
        }

        const _: () = assert!(
            (cfg!(target_pointer_width = "64") && std::mem::size_of::<INPUT>() == 40)
                || (cfg!(target_pointer_width = "32") && std::mem::size_of::<INPUT>() == 28)
        );

        extern "system" {
            fn SendInput(cInputs: u32, pInputs: *const INPUT, cbSize: i32) -> u32;
        }

        const INPUT_KEYBOARD: u32 = 1;
        const KEYEVENTF_KEYUP: u32 = 0x0002;
        const VK_CONTROL: u16 = 0x11;
        const VK_V: u16 = 0x56;

        unsafe {
            let inputs = [
                // Ctrl Down
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    u: KEYBDINPUT {
                        wVk: VK_CONTROL,
                        wScan: 0,
                        dwFlags: 0,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                    _pad: [0; 8],
                },
                // V Down
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    u: KEYBDINPUT {
                        wVk: VK_V,
                        wScan: 0,
                        dwFlags: 0,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                    _pad: [0; 8],
                },
                // V Up
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    u: KEYBDINPUT {
                        wVk: VK_V,
                        wScan: 0,
                        dwFlags: KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                    _pad: [0; 8],
                },
                // Ctrl Up
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    u: KEYBDINPUT {
                        wVk: VK_CONTROL,
                        wScan: 0,
                        dwFlags: KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                    _pad: [0; 8],
                },
            ];

            SendInput(4, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
        }
    }

    extern "system" {
        fn RegisterClipboardFormatW(lpszFormat: *const u16) -> u32;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
    }

    pub(super) fn is_clipboard_concealed() -> bool {
        static EXCLUDE_FMT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        static IGNORE_FMT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

        const EXCLUDE_NAME: &[u16] = &[
            b'E' as u16,
            b'x' as u16,
            b'c' as u16,
            b'l' as u16,
            b'u' as u16,
            b'd' as u16,
            b'e' as u16,
            b'C' as u16,
            b'l' as u16,
            b'i' as u16,
            b'p' as u16,
            b'b' as u16,
            b'o' as u16,
            b'a' as u16,
            b'r' as u16,
            b'd' as u16,
            b'C' as u16,
            b'o' as u16,
            b'n' as u16,
            b't' as u16,
            b'e' as u16,
            b'n' as u16,
            b't' as u16,
            b'F' as u16,
            b'r' as u16,
            b'o' as u16,
            b'm' as u16,
            b'M' as u16,
            b'o' as u16,
            b'n' as u16,
            b'i' as u16,
            b't' as u16,
            b'o' as u16,
            b'r' as u16,
            b'P' as u16,
            b'r' as u16,
            b'o' as u16,
            b'c' as u16,
            b'e' as u16,
            b's' as u16,
            b's' as u16,
            b'i' as u16,
            b'n' as u16,
            b'g' as u16,
            0,
        ];
        const IGNORE_NAME: &[u16] = &[
            b'C' as u16,
            b'l' as u16,
            b'i' as u16,
            b'p' as u16,
            b'b' as u16,
            b'o' as u16,
            b'a' as u16,
            b'r' as u16,
            b'd' as u16,
            b' ' as u16,
            b'V' as u16,
            b'i' as u16,
            b'e' as u16,
            b'w' as u16,
            b'e' as u16,
            b'r' as u16,
            b' ' as u16,
            b'I' as u16,
            b'g' as u16,
            b'n' as u16,
            b'o' as u16,
            b'r' as u16,
            b'e' as u16,
            0,
        ];

        let mut exclude_id = EXCLUDE_FMT.load(Ordering::Relaxed);
        if exclude_id == 0 {
            unsafe {
                exclude_id = RegisterClipboardFormatW(EXCLUDE_NAME.as_ptr());
                if exclude_id != 0 {
                    EXCLUDE_FMT.store(exclude_id, Ordering::Relaxed);
                }
            }
        }
        if exclude_id != 0 && unsafe { IsClipboardFormatAvailable(exclude_id) } != 0 {
            return true;
        }

        let mut ignore_id = IGNORE_FMT.load(Ordering::Relaxed);
        if ignore_id == 0 {
            unsafe {
                ignore_id = RegisterClipboardFormatW(IGNORE_NAME.as_ptr());
                if ignore_id != 0 {
                    IGNORE_FMT.store(ignore_id, Ordering::Relaxed);
                }
            }
        }
        if ignore_id != 0 && unsafe { IsClipboardFormatAvailable(ignore_id) } != 0 {
            return true;
        }

        false
    }

    /// Returns `true` when the current Windows keyboard layout produces `ù` for the
    /// physical key in the US `Quote` position (French/Belgian AZERTY layouts).
    pub(super) fn quote_key_is_ugrave() -> bool {
        extern "system" {
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

        const MAPVK_VSC_TO_VK: u32 = 1;

        unsafe {
            let hkl = GetKeyboardLayout(0);
            if hkl.is_null() {
                return false;
            }
            let vk = MapVirtualKeyExW(0x0028, MAPVK_VSC_TO_VK, hkl);
            if vk == 0 {
                return false;
            }
            let key_state = [0u8; 256];
            let mut buf = [0u16; 4];
            let rc = ToUnicodeEx(vk, 0x0028, key_state.as_ptr(), buf.as_mut_ptr(), buf.len() as i32, 0, hkl);
            if rc > 0 {
                let text = String::from_utf16_lossy(&buf[..rc as usize]);
                return text.chars().any(|c| c == 'ù' || c == 'Ù');
            }
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Linux Implementation (X11 / EWMH + XTest)
// ---------------------------------------------------------------------------
#[cfg(target_os = "linux")]
mod linux {
    use std::sync::atomic::{AtomicU32, Ordering};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        AtomEnum, ClientMessageEvent, ConnectionExt, EventMask, InputFocus, Window,
        CLIENT_MESSAGE_EVENT,
    };
    use x11rb::protocol::xtest::ConnectionExt as XTestConnectionExt;

    static PREVIOUS_X11_WINDOW: AtomicU32 = AtomicU32::new(0);

    fn active_window_and_pid() -> Option<(Window, Option<u32>)> {
        let (conn, screen_num) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots.get(screen_num)?.root;

        let active_atom = conn
            .intern_atom(true, b"_NET_ACTIVE_WINDOW")
            .ok()?
            .reply()
            .ok()?
            .atom;
        if active_atom == 0 {
            return None;
        }

        let active_reply = conn
            .get_property(false, root, active_atom, AtomEnum::WINDOW, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        let win = active_reply.value32()?.next()?;
        if win == 0 {
            return None;
        }

        let pid_atom = conn
            .intern_atom(true, b"_NET_WM_PID")
            .ok()
            .and_then(|c| c.reply().ok())
            .map_or(0, |r| r.atom);

        let pid = if pid_atom == 0 {
            None
        } else {
            conn.get_property(false, win, pid_atom, AtomEnum::CARDINAL, 0, 1)
                .ok()
                .and_then(|c| c.reply().ok())
                .and_then(|r| r.value32()?.next())
        };

        Some((win, pid))
    }

    pub(super) fn is_active() -> Option<bool> {
        let (_win, pid) = active_window_and_pid()?;
        pid.map(|p| p == std::process::id())
    }

    pub(super) fn capture_frontmost() {
        if let Some((win, pid)) = active_window_and_pid() {
            if pid != Some(std::process::id()) {
                PREVIOUS_X11_WINDOW.store(win, Ordering::SeqCst);
            }
        }
    }

    pub(super) fn restore_focus() {
        let win = PREVIOUS_X11_WINDOW.load(Ordering::SeqCst);
        if win == 0 {
            return;
        }

        let Ok((conn, screen_num)) = x11rb::connect(None) else {
            return;
        };
        let Some(root) = conn.setup().roots.get(screen_num).map(|s| s.root) else {
            return;
        };

        if let Ok(cookie) = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW") {
            if let Ok(reply) = cookie.reply() {
                if reply.atom != 0 {
                    let event = ClientMessageEvent {
                        response_type: CLIENT_MESSAGE_EVENT,
                        format: 32,
                        sequence: 0,
                        window: win,
                        type_: reply.atom,
                        data: [1, x11rb::CURRENT_TIME, 0, 0, 0].into(),
                    };
                    let _ = conn.send_event(
                        false,
                        root,
                        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                        event,
                    );
                }
            }
        }

        let _ = conn.set_input_focus(InputFocus::PARENT, win, x11rb::CURRENT_TIME);
        let _ = conn.flush();
    }

    fn find_keycode<C: Connection>(conn: &C, target_keysym: u32) -> Option<u8> {
        let min_kc = conn.setup().min_keycode;
        let max_kc = conn.setup().max_keycode;
        let count = max_kc.checked_sub(min_kc)?.saturating_add(1);
        let mapping = conn
            .get_keyboard_mapping(min_kc, count)
            .ok()?
            .reply()
            .ok()?;
        let per_kc = usize::from(mapping.keysyms_per_keycode);
        if per_kc == 0 {
            return None;
        }
        for (idx, chunk) in mapping.keysyms.chunks(per_kc).enumerate() {
            if chunk.contains(&target_keysym) {
                let offset = u8::try_from(idx).ok()?;
                return min_kc.checked_add(offset);
            }
        }
        None
    }

    pub(super) fn simulate_paste() {
        const XK_CONTROL_L: u32 = 0xffe3;
        const XK_V_LOWER: u32 = 0x0076;
        const KEY_PRESS: u8 = 2;
        const KEY_RELEASE: u8 = 3;

        let Ok((conn, screen_num)) = x11rb::connect(None) else {
            return;
        };
        let Some(root) = conn.setup().roots.get(screen_num).map(|s| s.root) else {
            return;
        };
        let Some(ctrl_kc) = find_keycode(&conn, XK_CONTROL_L) else {
            return;
        };
        let Some(v_kc) = find_keycode(&conn, XK_V_LOWER) else {
            return;
        };

        let _ = conn.xtest_fake_input(KEY_PRESS, ctrl_kc, 0, root, 0, 0, 0);
        let _ = conn.xtest_fake_input(KEY_PRESS, v_kc, 0, root, 0, 0, 0);
        let _ = conn.flush();

        std::thread::sleep(std::time::Duration::from_millis(15));

        let _ = conn.xtest_fake_input(KEY_RELEASE, v_kc, 0, root, 0, 0, 0);
        let _ = conn.xtest_fake_input(KEY_RELEASE, ctrl_kc, 0, root, 0, 0, 0);
        let _ = conn.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_capture_and_restore_no_panic() {
        // Calling capture and restore should never panic on any platform
        capture_active_window();
        restore_focus_and_paste(false, false, 120);
        assert!(is_accessibility_trusted() || !is_accessibility_trusted());
        let _ = is_app_active();
        let _ = is_clipboard_concealed();
        let _ = take_reopen_requested();
        hide_application();
        order_front_main_window();
        setup_reopen_observer();
    }
}
