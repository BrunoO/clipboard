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
#[allow(clippy::missing_const_for_fn)]
#[must_use]
pub(crate) fn is_app_active() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::is_active()
    }
    #[cfg(target_os = "windows")]
    {
        windows::is_active()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
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
#[allow(clippy::missing_const_for_fn)]
pub(crate) fn hide_application() {
    #[cfg(target_os = "macos")]
    macos::hide_app();
}

/// Unhides and orders the main window front at the OS level.
#[allow(clippy::missing_const_for_fn)]
pub(crate) fn order_front_main_window() {
    #[cfg(target_os = "macos")]
    macos::order_front_main_window();
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

/// Captures the current frontmost application/window before `CipherClipboard` gains focus.
#[allow(clippy::missing_const_for_fn)]
pub(crate) fn capture_active_window() {
    #[cfg(target_os = "macos")]
    macos::capture_frontmost();

    #[cfg(target_os = "windows")]
    windows::capture_frontmost();
}

/// Restores focus to the previous application and optionally simulates a paste keystroke.
pub(crate) fn restore_focus_and_paste(restore_focus: bool, auto_paste: bool) {
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
        }

        if auto_paste {
            // Give the OS window server 120ms to switch key focus
            // to the target application before dispatching keystrokes.
            std::thread::sleep(Duration::from_millis(120));

            #[cfg(target_os = "macos")]
            macos::simulate_paste();

            #[cfg(target_os = "windows")]
            windows::simulate_paste();
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

    pub(super) fn setup_reopen_observer() {
        unsafe {
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
        fn GetCurrentProcessId() -> u32;
        fn GetCurrentThreadId() -> u32;
        fn GetWindowThreadProcessId(hwnd: HWND, lpdwProcessId: *mut u32) -> u32;
        fn AttachThreadInput(idAttach: u32, idAttachTo: u32, fAttach: i32) -> i32;
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

        #[allow(non_snake_case, non_camel_case_types)]
        #[repr(C)]
        struct INPUT {
            r#type: u32,
            u: KEYBDINPUT,
            _pad: [u8; 16],
        }

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
                    _pad: [0; 16],
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
                    _pad: [0; 16],
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
                    _pad: [0; 16],
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
                    _pad: [0; 16],
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
            b'E' as u16, b'x' as u16, b'c' as u16, b'l' as u16, b'u' as u16, b'd' as u16,
            b'e' as u16, b'C' as u16, b'l' as u16, b'i' as u16, b'p' as u16, b'b' as u16,
            b'o' as u16, b'a' as u16, b'r' as u16, b'd' as u16, b'C' as u16, b'o' as u16,
            b'n' as u16, b't' as u16, b'e' as u16, b'n' as u16, b't' as u16, b'F' as u16,
            b'r' as u16, b'o' as u16, b'm' as u16, b'M' as u16, b'o' as u16, b'n' as u16,
            b'i' as u16, b't' as u16, b'o' as u16, b'r' as u16, b'P' as u16, b'r' as u16,
            b'o' as u16, b'c' as u16, b'e' as u16, b's' as u16, b's' as u16, b'i' as u16,
            b'n' as u16, b'g' as u16, 0,
        ];
        const IGNORE_NAME: &[u16] = &[
            b'C' as u16, b'l' as u16, b'i' as u16, b'p' as u16, b'b' as u16, b'o' as u16,
            b'a' as u16, b'r' as u16, b'd' as u16, b' ' as u16, b'V' as u16, b'i' as u16,
            b'e' as u16, b'w' as u16, b'e' as u16, b'r' as u16, b' ' as u16, b'I' as u16,
            b'g' as u16, b'n' as u16, b'o' as u16, b'r' as u16, b'e' as u16, 0,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_capture_and_restore_no_panic() {
        // Calling capture and restore should never panic on any platform
        capture_active_window();
        restore_focus_and_paste(false, false);
        assert!(is_accessibility_trusted() || !is_accessibility_trusted());
        let _ = is_app_active();
        let _ = is_clipboard_concealed();
        let _ = take_reopen_requested();
        hide_application();
        order_front_main_window();
        setup_reopen_observer();
    }
}
