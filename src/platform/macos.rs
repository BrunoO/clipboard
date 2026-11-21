#![allow(unsafe_code)]
#![allow(unexpected_cfgs)]

use objc::declare::ClassDecl;
use objc::runtime::{Object, Sel};
use objc::{class, msg_send, sel, sel_impl};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Mutex;

static PREVIOUS_APP_PID: AtomicI32 = AtomicI32::new(0);
static REPAINT_CTX: Mutex<Option<eframe::egui::Context>> = Mutex::new(None);

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

static REOPEN_REQUESTED: AtomicBool = AtomicBool::new(false);

pub(super) fn take_reopen_requested() -> bool {
    REOPEN_REQUESTED.swap(false, Ordering::AcqRel)
}

/// `AppKit`'s `[NSApplication sharedApplication]` asserts it is called on the
/// main thread (trapping with `SIGTRAP` on macOS if invoked from a background
/// or `cargo test` worker thread).
fn is_main_thread() -> bool {
    // SAFETY: NSThread's `isMainThread` is a side-effect-free class query; the receiver comes from the ObjC runtime and no shared state is mutated.
    unsafe {
        let is_main: bool = msg_send![class!(NSThread), isMainThread];
        is_main
    }
}

pub(super) fn is_active() -> bool {
    if !is_main_thread() {
        return false;
    }
    // SAFETY: Guarded by `is_main_thread()`; `sharedApplication` is null-checked before messaging, and these AppKit getters do not mutate private windows.
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
    // SAFETY: Guarded by `is_main_thread()`; `sharedApplication` is null-checked and `hide:` accepts nil, so only a valid app object receives a valid selector.
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
    // SAFETY: Main-thread guarded and autoreleased; every AppKit object is null-checked, and private windows are skipped via `canBecomeMainWindow`.
    objc::rc::autoreleasepool(|| unsafe {
        let ns_app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        if ns_app.is_null() {
            return;
        }
        let nil_obj: *mut Object = std::ptr::null_mut();
        let hidden: bool = msg_send![ns_app, isHidden];
        if hidden {
            let _: () = msg_send![ns_app, unhide: nil_obj];
        }
        let active: bool = msg_send![ns_app, isActive];
        if !active {
            let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];
        }

        let windows: *mut Object = msg_send![ns_app, windows];
        if !windows.is_null() {
            let count: usize = msg_send![windows, count];
            for i in 0..count {
                let window: *mut Object = msg_send![windows, objectAtIndex: i];
                if window.is_null() {
                    continue;
                }
                // Skip AppKit's private internal windows (Touch Bar panels,
                // tooltips, popovers, animation proxies) — mutating their
                // visibility/key state corrupts _NSTouchBarFinderObservation KVO.
                let can_main: bool = msg_send![window, canBecomeMainWindow];
                if can_main {
                    let _: () = msg_send![window, makeKeyAndOrderFront: nil_obj];
                }
            }
        }
    });
}

extern "C" fn on_app_reopen(_this: &Object, _cmd: Sel, _notification: *mut Object) {
    // Defer window ordering to the next egui frame (ClipboardApp::update checks
    // take_reopen_requested()) instead of mutating AppKit windows re-entrantly
    // inside NSNotificationCenter activation/unhide callbacks.
    REOPEN_REQUESTED.store(true, Ordering::Release);
    request_repaint();
}

// Strictly main-thread-only (guarded by is_main_thread()), so a thread_local
// avoids needing unsafe Send/Sync impls or a Mutex.
thread_local! {
    static REOPEN_OBSERVER: std::cell::Cell<*mut Object> = const {
        std::cell::Cell::new(std::ptr::null_mut())
    };
}

pub(super) fn setup_reopen_observer() {
    static INIT: std::sync::Once = std::sync::Once::new();
    if !is_main_thread() {
        return;
    }
    INIT.call_once(|| {
        // SAFETY: Main-thread guarded, run once, and autoreleased; the class is registered before `class!`, both objects are null-checked, and the observer is retained for teardown.
        objc::rc::autoreleasepool(|| unsafe {
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
            REOPEN_OBSERVER.with(|cell| cell.set(observer));
        });
    });
}

pub(super) fn teardown_reopen_observer() {
    if !is_main_thread() {
        return;
    }
    let observer = REOPEN_OBSERVER.with(|cell| cell.replace(std::ptr::null_mut()));
    if !observer.is_null() {
        // SAFETY: Main-thread guarded and autoreleased; the observer pointer came from `setup_reopen_observer`, the center is null-checked, and `release` balances `new`.
        objc::rc::autoreleasepool(|| unsafe {
            let center: *mut Object = msg_send![class!(NSNotificationCenter), defaultCenter];
            if !center.is_null() {
                let _: () = msg_send![center, removeObserver: observer];
            }
            let _: () = msg_send![observer, release];
        });
    }
    // Release the cloned egui::Context held in the global static on shutdown.
    let mut lock = REPAINT_CTX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *lock = None;
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

pub(crate) fn is_trusted() -> bool {
    // SAFETY: `AXIsProcessTrusted` takes no pointers and only queries a global permission flag; no object lifetime is involved.
    unsafe { AXIsProcessTrusted() }
}

pub(crate) fn prompt_permission() -> bool {
    // SAFETY: Autoreleased; the NSString/NSNumber/NSDictionary objects are null-checked and stay alive for the call, and the NSDictionary is CF-compatible for `AXIsProcessTrustedWithOptions`.
    objc::rc::autoreleasepool(|| unsafe {
        let str_obj: *mut Object = msg_send![
            class!(NSString),
            stringWithUTF8String: c"AXTrustedCheckOptionPrompt".as_ptr()
        ];
        let num_obj: *mut Object = msg_send![class!(NSNumber), numberWithBool: true];
        if str_obj.is_null() || num_obj.is_null() {
            return AXIsProcessTrusted();
        }
        let dict: *mut Object =
            msg_send![class!(NSDictionary), dictionaryWithObject: num_obj forKey: str_obj];
        if dict.is_null() {
            return AXIsProcessTrusted();
        }
        AXIsProcessTrustedWithOptions(dict as *const std::ffi::c_void)
    })
}

pub(crate) fn open_accessibility_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

pub(super) fn capture_frontmost() {
    // SAFETY: Autoreleased; `NSWorkspace` and its frontmost app are null-checked before messaging, and PIDs are converted/compared without truncation.
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
        let Ok(our_pid) = i32::try_from(std::process::id()) else {
            return;
        };

        if pid > 0 && pid != our_pid {
            PREVIOUS_APP_PID.store(pid, Ordering::Relaxed);
        }
    });
}

pub(super) fn restore_focus() {
    // Note: hide_app() is already called on the main thread before spawning
    // the restore/paste worker thread. Calling [NSApplication sharedApplication]
    // from a background thread violates AppKit's main-thread contract (SIGTRAP).
    // SAFETY: Autoreleased; the PID comes from our own atomic, the `NSRunningApplication` result is null-checked, and no `NSApplication` call is made off the main thread.
    objc::rc::autoreleasepool(|| unsafe {
        let pid = PREVIOUS_APP_PID.load(Ordering::Relaxed);
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
    let target_pid = PREVIOUS_APP_PID.load(Ordering::Relaxed);
    if target_pid <= 0 {
        return false;
    }
    // SAFETY: Autoreleased; workspace/frontmost app are null-checked before reading the PID scalar used for the foreground comparison.
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

    // SAFETY: All CoreGraphics constants are fixed; event and source handles are null-checked before flags/post/release, and only handles created here are released.
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

pub(super) fn clipboard_change_token() -> Option<u64> {
    // SAFETY: Autoreleased; the pasteboard is null-checked and `changeCount` is read as a scalar inside the pool's lifetime.
    objc::rc::autoreleasepool(|| unsafe {
        let pasteboard: *mut Object = msg_send![class!(NSPasteboard), generalPasteboard];
        if pasteboard.is_null() {
            return None;
        }
        let count: isize = msg_send![pasteboard, changeCount];
        Some((count as i64).cast_unsigned())
    })
}

pub(super) fn is_clipboard_concealed() -> bool {
    // SAFETY: Autoreleased; pasteboard, types, and each item are null-checked, and `UTF8String` data is read as a `CStr` only while the owning NSString is alive.
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
    // SAFETY: Autoreleased; pasteboard and NSData are null-checked, each marker is NUL-terminated, and type arrays are null-checked before being passed to AppKit.
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
            c"org.nspasteboard.ConcealedType",
            c"org.nspasteboard.TransientType",
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
