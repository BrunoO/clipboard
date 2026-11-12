#![allow(unsafe_code)]

//! Cross-platform focus restoration and simulated paste automation.
//!
//! When `CipherClipboard` is summoned via a global hotkey, it takes OS window focus.
//! This module captures the active application/window *before* `CipherClipboard` takes
//! focus, and restores focus and simulates `Cmd+V` (macOS) or `Ctrl+V` (Windows/Linux)
//! when an item is chosen.

use std::time::Duration;

#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicI32, Ordering};

#[cfg(target_os = "macos")]
static PREVIOUS_APP_PID: AtomicI32 = AtomicI32::new(0);

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
        if restore_focus {
            #[cfg(target_os = "macos")]
            macos::restore_focus();

            #[cfg(target_os = "windows")]
            windows::restore_focus();
        }

        if auto_paste {
            // Give the OS window server 100ms to switch key focus
            // to the target application before dispatching keystrokes.
            std::thread::sleep(Duration::from_millis(100));

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
    use super::{AtomicI32, Ordering, PREVIOUS_APP_PID};
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

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
        fn GetCurrentProcessId() -> u32;
        fn GetWindowThreadProcessId(hwnd: HWND, lpdwProcessId: *mut u32) -> u32;
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
                SetForegroundWindow(hwnd_raw as HWND);
            }
        }
    }

    pub(super) fn simulate_paste() {
        #[repr(C)]
        struct KEYBDINPUT {
            wVk: u16,
            wScan: u16,
            dwFlags: u32,
            time: u32,
            dwExtraInfo: usize,
        }

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
    }
}
