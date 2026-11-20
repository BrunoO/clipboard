#![allow(unsafe_code)]
#![allow(unexpected_cfgs)]
#![allow(clippy::missing_const_for_fn)]

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
#[cfg(test)]
#[must_use]
pub(crate) fn is_app_active() -> bool {
    is_app_active_state().unwrap_or(false)
}

/// Returns `Some(true)` if this application is currently active at the OS/WM level,
/// `Some(false)` if another application is active, or `None` if the platform/session
/// cannot determine active-window ownership (e.g. pure Wayland without X11).
#[must_use]
#[cfg_attr(
    any(target_os = "macos", target_os = "windows"),
    allow(clippy::unnecessary_wraps)
)]
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

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn windows_target_is_elevated() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::target_is_elevated()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

pub(crate) fn windows_set_title_bar_dark_mode(is_dark: bool) {
    #[cfg(target_os = "windows")]
    {
        windows::set_title_bar_dark_mode(is_dark);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = is_dark;
    }
}

const MAX_LOG_BYTES: u64 = 1024 * 1024; // Rotate app.log once it reaches 1 MiB

pub(crate) fn log_message(level: &str, message: &str) {
    use std::io::Write;
    if let Some(base) = dirs::data_local_dir() {
        let dir = base.join("cipherclipboard");
        let _ = std::fs::create_dir_all(&dir);
        crate::crypto::set_restricted_dir_permissions(&dir);
        let path = dir.join("app.log");
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.len() >= MAX_LOG_BYTES {
                let backup = dir.join("app.log.old");
                let _ = std::fs::remove_file(&backup);
                let _ = std::fs::rename(&path, &backup);
            }
        }
        let mut opts = std::fs::OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        if let Ok(mut file) = opts.open(&path) {
            let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(file, "[{ts}] [{level}] {message}");
        }
    }
}

/// Checks if the current clipboard content is marked as concealed or transient
/// by password managers (e.g. 1Password, Bitwarden, `KeePass`, `KeePassXC`, macOS Keychain).
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
    #[cfg(target_os = "linux")]
    {
        linux::is_clipboard_concealed()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        false
    }
}

/// Attaches OS-native privacy/concealed markers to the current clipboard entry
/// so OS clipboard history, cloud sync, Universal Clipboard, and other clipboard
/// managers do not record or broadcast secrets copied by `CipherClipboard`.
pub(crate) fn mark_clipboard_concealed() {
    #[cfg(target_os = "macos")]
    macos::mark_clipboard_concealed();

    #[cfg(target_os = "windows")]
    windows::mark_clipboard_concealed();
}

/// Hides the entire application at the OS level (macOS `[NSApp hide:]`).
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub(crate) fn hide_application() {
    #[cfg(target_os = "macos")]
    macos::hide_app();
}

/// Unhides and orders the main window front at the OS level.
pub(crate) fn order_front_main_window() {
    #[cfg(target_os = "macos")]
    macos::order_front_main_window();

    #[cfg(target_os = "windows")]
    windows::order_front_main_window();
}

/// On Windows, activates and brings to front any existing running instance window,
/// matching `expected_pid` when known to prevent title-based window spoofing.
#[cfg(target_os = "windows")]
pub(crate) fn activate_existing_instance(expected_pid: Option<u32>) -> bool {
    windows::activate_existing_instance(expected_pid)
}

/// Displays a native modal error dialog on Windows before terminating.
#[cfg(target_os = "windows")]
pub(crate) fn show_error_dialog(message: &str) {
    windows::show_error_dialog(message);
}

/// Checks and clears any pending reopen requests triggered by OS events (e.g. Dock icon click).
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
pub(crate) fn setup_reopen_observer() {
    #[cfg(target_os = "macos")]
    macos::setup_reopen_observer();
}

/// Removes the macOS reopen observer to prevent crashes on app termination.
pub(crate) fn teardown_reopen_observer() {
    #[cfg(target_os = "macos")]
    macos::teardown_reopen_observer();
}

/// Captures the current frontmost application/window before `CipherClipboard` gains focus.
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
            {
                if !macos::is_target_foreground() {
                    crate::platform::log_message(
                        "WARN",
                        "Auto-paste skipped: captured target application is not frontmost",
                    );
                    return;
                }
                macos::simulate_paste();
            }

            #[cfg(target_os = "windows")]
            {
                if !windows::is_target_foreground() {
                    crate::platform::log_message(
                        "WARN",
                        "Auto-paste skipped: captured target window is not in the foreground",
                    );
                    return;
                }
                let _ = windows::simulate_paste();
            }

            #[cfg(target_os = "linux")]
            {
                if !linux::is_target_foreground() {
                    crate::platform::log_message(
                        "WARN",
                        "Auto-paste skipped: captured target window is not active",
                    );
                    return;
                }
                linux::simulate_paste();
            }
        }
    });
}

/// Checks whether macOS Accessibility permissions are granted for synthetic event posting.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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

    /// `AppKit`'s `[NSApplication sharedApplication]` asserts it is called on the
    /// main thread (trapping with `SIGTRAP` on macOS if invoked from a background
    /// or `cargo test` worker thread).
    fn is_main_thread() -> bool {
        unsafe {
            let is_main: bool = msg_send![class!(NSThread), isMainThread];
            is_main
        }
    }

    pub(super) fn is_active() -> bool {
        if !is_main_thread() {
            return false;
        }
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
        if !is_main_thread() {
            return;
        }
        unsafe {
            let ns_app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            if !ns_app.is_null() {
                let nil_obj: *mut Object = std::ptr::null_mut();
                let _: () = msg_send![ns_app, hide: nil_obj];
            }
        }
    }

    pub(super) fn order_front_main_window() {
        if !is_main_thread() {
            return;
        }
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
                stringWithUTF8String: c"NSApplicationDidBecomeActiveNotification".as_ptr()
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
                stringWithUTF8String: c"NSApplicationDidUnhideNotification".as_ptr()
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
        let observer = REOPEN_OBSERVER.lock().ok().and_then(|mut lock| lock.take());
        if let Some(ObserverPtr(observer)) = observer {
            objc::rc::autoreleasepool(|| unsafe {
                let center: *mut Object = msg_send![class!(NSNotificationCenter), defaultCenter];
                if !center.is_null() {
                    let _: () = msg_send![center, removeObserver: observer];
                }
                let _: () = msg_send![observer, release];
            });
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
        objc::rc::autoreleasepool(|| unsafe {
            let key = b"AXTrustedCheckOptionPrompt\0";
            let str_obj: *mut Object =
                msg_send![class!(NSString), stringWithUTF8String: key.as_ptr()];
            let num_obj: *mut Object = msg_send![class!(NSNumber), numberWithBool: true];
            let dict: *mut Object =
                msg_send![class!(NSDictionary), dictionaryWithObject: num_obj forKey: str_obj];
            AXIsProcessTrustedWithOptions(dict as *const std::ffi::c_void)
        })
    }

    pub(super) fn open_accessibility_settings() {
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn();
    }

    pub(super) fn capture_frontmost() {
        objc::rc::autoreleasepool(|| unsafe {
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
        });
    }

    pub(super) fn restore_focus() {
        // Note: hide_app() is already called on the main thread before spawning
        // the restore/paste worker thread. Calling [NSApplication sharedApplication]
        // from a background thread violates AppKit's main-thread contract (SIGTRAP).
        objc::rc::autoreleasepool(|| unsafe {
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
        });
    }

    pub(super) fn is_target_foreground() -> bool {
        let target_pid = PREVIOUS_APP_PID.load(Ordering::SeqCst);
        if target_pid <= 0 {
            return false;
        }
        objc::rc::autoreleasepool(|| unsafe {
            let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
            if workspace.is_null() {
                return false;
            }
            let frontmost: *mut Object = msg_send![workspace, frontmostApplication];
            if frontmost.is_null() {
                return false;
            }
            let pid: i32 = msg_send![frontmost, processIdentifier];
            pid == target_pid
        })
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
        objc::rc::autoreleasepool(|| unsafe {
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
        })
    }

    pub(super) fn mark_clipboard_concealed() {
        objc::rc::autoreleasepool(|| unsafe {
            let pasteboard: *mut Object = msg_send![class!(NSPasteboard), generalPasteboard];
            if pasteboard.is_null() {
                return;
            }
            let empty_data: *mut Object = msg_send![class!(NSData), data];
            if empty_data.is_null() {
                return;
            }
            let nil_obj: *mut Object = std::ptr::null_mut();
            for marker in [
                b"org.nspasteboard.ConcealedType\0".as_slice(),
                b"org.nspasteboard.TransientType\0".as_slice(),
            ] {
                let type_str: *mut Object =
                    msg_send![class!(NSString), stringWithUTF8String: marker.as_ptr()];
                if type_str.is_null() {
                    continue;
                }
                let types_arr: *mut Object = msg_send![class!(NSArray), arrayWithObject: type_str];
                if !types_arr.is_null() {
                    let _: isize = msg_send![pasteboard, addTypes: types_arr owner: nil_obj];
                    let _: bool = msg_send![pasteboard, setData: empty_data forType: type_str];
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Windows Implementation
// ---------------------------------------------------------------------------
#[cfg(target_os = "windows")]
mod windows {
    use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

    use super::log_message;

    static PREVIOUS_HWND: AtomicIsize = AtomicIsize::new(0);
    static PREVIOUS_PID: AtomicU32 = AtomicU32::new(0);

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
        fn EnumWindows(
            lpEnumFunc: unsafe extern "system" fn(Hwnd, isize) -> i32,
            lParam: isize,
        ) -> i32;
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
            GetWindowThreadProcessId(fg, &mut fg_pid)
        };

        let target_thread = if fg_thread != 0 && fg_pid != current_pid {
            fg_thread
        } else {
            let hwnd_raw = PREVIOUS_HWND.load(Ordering::SeqCst);
            if hwnd_raw != 0 {
                let hwnd = hwnd_raw as Hwnd;
                if IsWindow(hwnd) != 0 {
                    GetWindowThreadProcessId(hwnd, std::ptr::null_mut())
                } else {
                    0
                }
            } else {
                0
            }
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
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == GetCurrentProcessId() {
                let mut buf = [0u16; 64];
                let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
                if len > 0 {
                    let title = String::from_utf16_lossy(&buf[..len as usize]);
                    if title == "CipherClipboard" {
                        let out = lparam as *mut Hwnd;
                        if !out.is_null() {
                            *out = hwnd;
                        }
                        return 0; // stop enumeration
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
                if pid != 0 && pid != GetCurrentProcessId() {
                    PREVIOUS_HWND.store(hwnd as isize, Ordering::SeqCst);
                    PREVIOUS_PID.store(pid, Ordering::SeqCst);
                }
            }
        }
    }

    pub(super) fn restore_focus() {
        let hwnd_raw = PREVIOUS_HWND.load(Ordering::SeqCst);
        if hwnd_raw == 0 {
            log_message(
                "WARN",
                "Focus restore skipped: no previously captured window",
            );
            return;
        }

        unsafe {
            let target_hwnd = hwnd_raw as Hwnd;
            if IsWindow(target_hwnd) == 0 {
                PREVIOUS_HWND.store(0, Ordering::SeqCst);
                PREVIOUS_PID.store(0, Ordering::SeqCst);
                log_message(
                    "WARN",
                    "Focus restore skipped: captured target window no longer exists",
                );
                return;
            }

            // A bare worker thread has no message queue and AttachThreadInput fails
            // unless both threads have one. PeekMessage creates it as a side effect.
            let mut msg = std::mem::zeroed::<MSG>();
            let _ = PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);

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
        let hwnd_raw = PREVIOUS_HWND.load(Ordering::SeqCst);
        if hwnd_raw == 0 {
            return false;
        }
        unsafe {
            let fg = GetForegroundWindow();
            if fg.is_null() {
                return false;
            }
            if fg == hwnd_raw as Hwnd {
                return true;
            }
            // Some apps replace their top-level Hwnd between capture and paste.
            // Accepting the captured process is still safe: never another app.
            let target_pid = PREVIOUS_PID.load(Ordering::SeqCst);
            if target_pid == 0 {
                return false;
            }
            let mut fg_pid: u32 = 0;
            GetWindowThreadProcessId(fg, &mut fg_pid);
            fg_pid != 0 && fg_pid == target_pid
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
            GetWindowThreadProcessId(hwnd, &mut pid);
            if let Some(expected) = ctx.expected_pid {
                if pid != expected {
                    return 1;
                }
            }
            let mut buf = [0u16; 64];
            let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            if len > 0 {
                let title = String::from_utf16_lossy(&buf[..len as usize]);
                if title == "CipherClipboard" {
                    ctx.found = hwnd;
                    return 0;
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
            if !ctx.found.is_null() {
                // SW_RESTORE on a maximized/snapped window would un-maximize it.
                if IsIconic(ctx.found) != 0 {
                    ShowWindow(ctx.found, SW_RESTORE);
                } else {
                    ShowWindow(ctx.found, SW_SHOW);
                }
                BringWindowToTop(ctx.found);
                SetForegroundWindow(ctx.found);
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
                hwnd: Hwnd,
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

            let inserted = SendInput(4, inputs.as_ptr(), std::mem::size_of::<Input>() as i32);
            let ok = inserted == 4;
            if !ok {
                log_message(
                    "WARN",
                    &format!("SendInput inserted {inserted}/4 events for auto-paste (possible UIPI block)"),
                );
            }
            ok
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

        // Windows 10/11 Clipboard History API: apps set CanIncludeInClipboardHistory = DWORD(0)
        // to mark sensitive items as excluded from clipboard history.
        static HISTORY_FMT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        const HISTORY_NAME: &[u16] = &[
            b'C' as u16,
            b'a' as u16,
            b'n' as u16,
            b'I' as u16,
            b'n' as u16,
            b'c' as u16,
            b'l' as u16,
            b'u' as u16,
            b'd' as u16,
            b'e' as u16,
            b'I' as u16,
            b'n' as u16,
            b'C' as u16,
            b'l' as u16,
            b'i' as u16,
            b'p' as u16,
            b'b' as u16,
            b'o' as u16,
            b'a' as u16,
            b'r' as u16,
            b'd' as u16,
            b'H' as u16,
            b'i' as u16,
            b's' as u16,
            b't' as u16,
            b'o' as u16,
            b'r' as u16,
            b'y' as u16,
            0,
        ];

        let mut history_id = HISTORY_FMT.load(Ordering::Relaxed);
        if history_id == 0 {
            unsafe {
                history_id = RegisterClipboardFormatW(HISTORY_NAME.as_ptr());
                if history_id != 0 {
                    HISTORY_FMT.store(history_id, Ordering::Relaxed);
                }
            }
        }
        if history_id != 0 && unsafe { IsClipboardFormatAvailable(history_id) } != 0 {
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
                if OpenClipboard(std::ptr::null_mut()) != 0 {
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
                    if excluded {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Appends Windows privacy clipboard formats without clearing the text just written:
    /// - `ExcludeClipboardContentFromMonitorProcessing` (prevents clipboard monitors)
    /// - `CanIncludeInClipboardHistory` = 0 (blocks Win+V local history)
    /// - `CanUploadClipboardContentToCloud` = 0 (blocks Windows Cloud Clipboard sync)
    pub(super) fn mark_clipboard_concealed() {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        #[link(name = "user32")]
        extern "system" {
            fn OpenClipboard(hWndNewOwner: Hwnd) -> i32;
            fn CloseClipboard() -> i32;
            fn SetClipboardData(uFormat: u32, hMem: *mut std::ffi::c_void)
                -> *mut std::ffi::c_void;
        }

        #[link(name = "kernel32")]
        extern "system" {
            fn GlobalAlloc(uFlags: u32, dwBytes: usize) -> *mut std::ffi::c_void;
            fn GlobalLock(hMem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
            fn GlobalUnlock(hMem: *mut std::ffi::c_void) -> i32;
            fn GlobalFree(hMem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        }

        const GMEM_MOVEABLE: u32 = 0x0002;

        unsafe fn set_zero_dword_format(name: &str) {
            let wide: Vec<u16> = OsStr::new(name)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let fmt = RegisterClipboardFormatW(wide.as_ptr());
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
            set_zero_dword_format("ExcludeClipboardContentFromMonitorProcessing");
            set_zero_dword_format("CanIncludeInClipboardHistory");
            set_zero_dword_format("CanUploadClipboardContentToCloud");
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
                buf.len() as i32,
                0,
                hkl,
            );
            if rc > 0 {
                let text = String::from_utf16_lossy(&buf[..rc as usize]);
                return text.chars().any(|c| c == 'ù' || c == 'Ù');
            }
            false
        }
    }

    unsafe fn captured_target_pid() -> u32 {
        let hwnd_raw = PREVIOUS_HWND.load(Ordering::SeqCst);
        if hwnd_raw != 0 {
            let hwnd = hwnd_raw as Hwnd;
            if IsWindow(hwnd) != 0 {
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, &mut pid);
                if pid != 0 && pid != GetCurrentProcessId() {
                    return pid;
                }
            }
        }
        let stored = PREVIOUS_PID.load(Ordering::SeqCst);
        if stored != 0 && stored != GetCurrentProcessId() {
            stored
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
            let opened = OpenProcessToken(proc, TOKEN_QUERY, &mut token) != 0 && !token.is_null();
            let mut level = None;
            if opened {
                let mut buf = TokenIntegrityBuf([0u8; 256]);
                let mut ret_len = 0u32;
                let ok = GetTokenInformation(
                    token,
                    TOKEN_INTEGRITY_LEVEL_CLASS,
                    buf.0.as_mut_ptr().cast::<std::ffi::c_void>(),
                    u32::try_from(buf.0.len()).unwrap_or(256),
                    &mut ret_len,
                );
                if ok != 0 {
                    let info = &*buf.0.as_ptr().cast::<SidAndAttributes>();
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
            match integrity_level(target_pid) {
                Some(level) => level > self_level,
                // Protected / higher-integrity processes often refuse TOKEN_QUERY.
                // Fail closed: asking the user to paste manually is safer than a
                // silent drop by UIPI.
                None => true,
            }
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

    pub(super) fn is_target_foreground() -> bool {
        let target_win = PREVIOUS_X11_WINDOW.load(Ordering::SeqCst);
        if target_win == 0 {
            return false;
        }
        active_window_and_pid().is_some_and(|(win, _)| win == target_win)
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

    /// Detects concealed clipboard entries on X11 (e.g. `KeePassXC`, Bitwarden, 1Password)
    /// by inspecting the `CLIPBOARD` selection's advertised `TARGETS` for
    /// `x-kde-passwordManagerHint`.
    pub(super) fn is_clipboard_concealed() -> bool {
        use x11rb::protocol::xproto::{CreateWindowAux, WindowClass};

        let Ok((conn, screen_num)) = x11rb::connect(None) else {
            return false;
        };
        let Some(screen) = conn.setup().roots.get(screen_num) else {
            return false;
        };
        let root = screen.root;

        let intern = |name: &[u8]| -> Option<u32> {
            let atom = conn.intern_atom(false, name).ok()?.reply().ok()?.atom;
            (atom != 0).then_some(atom)
        };

        let Some(clipboard_atom) = intern(b"CLIPBOARD") else {
            return false;
        };
        let Some(targets_atom) = intern(b"TARGETS") else {
            return false;
        };
        let Some(hint_atom) = intern(b"x-kde-passwordManagerHint") else {
            return false;
        };
        let Some(prop_atom) = intern(b"_CIPHERCLIPBOARD_TARGETS") else {
            return false;
        };

        // Fast path: if nobody owns CLIPBOARD, there's nothing concealed.
        let Ok(owner_cookie) = conn.get_selection_owner(clipboard_atom) else {
            return false;
        };
        let Ok(owner_reply) = owner_cookie.reply() else {
            return false;
        };
        if owner_reply.owner == 0 {
            return false;
        }

        let Ok(win) = conn.generate_id() else {
            return false;
        };
        if conn
            .create_window(
                0,
                win,
                root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                0,
                &CreateWindowAux::new(),
            )
            .is_err()
        {
            return false;
        }

        let concealed = if conn
            .convert_selection(
                win,
                clipboard_atom,
                targets_atom,
                prop_atom,
                x11rb::CURRENT_TIME,
            )
            .is_ok()
            && conn.flush().is_ok()
        {
            poll_selection_for_hint(&conn, win, hint_atom)
        } else {
            false
        };

        let _ = conn.destroy_window(win);
        let _ = conn.flush();
        concealed
    }

    fn poll_selection_for_hint(
        conn: &x11rb::rust_connection::RustConnection,
        win: u32,
        hint_atom: u32,
    ) -> bool {
        use std::time::{Duration, Instant};
        use x11rb::protocol::Event;

        let deadline = Instant::now() + Duration::from_millis(50);
        while Instant::now() < deadline {
            match conn.poll_for_event() {
                Ok(Some(Event::SelectionNotify(ev))) => {
                    if ev.property == 0 {
                        return false;
                    }
                    return conn
                        .get_property(true, win, ev.property, AtomEnum::ATOM, 0, 1024)
                        .ok()
                        .and_then(|cookie| cookie.reply().ok())
                        .and_then(|reply| {
                            reply
                                .value32()
                                .map(|mut atoms| atoms.any(|a| a == hint_atom))
                        })
                        .unwrap_or(false);
                }
                Ok(Some(_)) => {}
                Ok(None) => std::thread::sleep(Duration::from_millis(2)),
                Err(_) => break,
            }
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
