#![allow(unsafe_code)]

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Mutex;

use objc2::rc::{autoreleasepool, Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, NSObject};
use objc2::{define_class, msg_send, sel, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSApplicationDidBecomeActiveNotification,
    NSApplicationDidUnhideNotification, NSPasteboard, NSRunningApplication, NSWorkspace,
};
use objc2_foundation::{
    ns_string, NSArray, NSData, NSDictionary, NSError, NSNotification, NSNotificationCenter,
    NSNumber, NSString,
};

static PREVIOUS_APP_PID: AtomicI32 = AtomicI32::new(0);
static REPAINT_CTX: Mutex<Option<eframe::egui::Context>> = Mutex::new(None);

/// Locks `mutex`, recovering and reporting poison instead of silently dropping state.
fn lock_or_recover<'a, T>(mutex: &'a Mutex<T>, site: &str) -> std::sync::MutexGuard<'a, T> {
    mutex.lock().unwrap_or_else(|poisoned| {
        crate::logging::log_message(
            "WARN",
            &format!("Recovered poisoned mutex at {site} (REPAINT_CTX)"),
        );
        poisoned.into_inner()
    })
}

pub(super) fn set_repaint_context(ctx: &eframe::egui::Context) {
    *lock_or_recover(&REPAINT_CTX, "set_repaint_context") = Some(ctx.clone());
}

fn request_repaint() {
    let lock = lock_or_recover(&REPAINT_CTX, "request_repaint");
    if let Some(ctx) = lock.as_ref() {
        ctx.request_repaint();
    }
}

static REOPEN_REQUESTED: AtomicBool = AtomicBool::new(false);

pub(super) fn take_reopen_requested() -> bool {
    REOPEN_REQUESTED.swap(false, Ordering::AcqRel)
}

pub(super) fn is_active() -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.isActive() && !ns_app.isHidden()
}

pub(super) fn hide_app() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.hide(None);
}

pub(super) fn order_front_main_window() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    autoreleasepool(|_| {
        let ns_app = NSApplication::sharedApplication(mtm);
        if ns_app.isHidden() {
            ns_app.unhide(None);
        }
        if !ns_app.isActive() {
            #[allow(deprecated)]
            ns_app.activateIgnoringOtherApps(true);
        }

        for window in ns_app.windows() {
            // Skip AppKit's private internal windows (Touch Bar panels,
            // tooltips, popovers, animation proxies) — mutating their
            // visibility/key state corrupts _NSTouchBarFinderObservation KVO.
            if window.canBecomeMainWindow() {
                window.makeKeyAndOrderFront(None);
            }
        }
    });
}

define_class!(
    // SAFETY:
    // - `NSObject` has no subclassing requirements.
    // - `ReopenObserver` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CipherReopenObserver"]
    struct ReopenObserver;

    impl ReopenObserver {
        #[unsafe(method(onNotification:))]
        fn on_notification(&self, _notification: &NSNotification) {
            // Defer window ordering to the next egui frame (ClipboardApp::update checks
            // take_reopen_requested()) instead of mutating AppKit windows re-entrantly
            // inside NSNotificationCenter activation/unhide callbacks.
            REOPEN_REQUESTED.store(true, Ordering::Release);
            request_repaint();
        }
    }
);

impl ReopenObserver {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `ReopenObserver` inherits from `NSObject` with unit ivars; `init` is valid.
        unsafe { msg_send![super(this), init] }
    }
}

// Strictly main-thread-only (guarded by `MainThreadMarker`), so a thread_local
// avoids needing unsafe Send/Sync impls or a Mutex.
thread_local! {
    static REOPEN_OBSERVER: RefCell<Option<Retained<ReopenObserver>>> = const {
        RefCell::new(None)
    };
}

pub(super) fn setup_reopen_observer() {
    static INIT: std::sync::Once = std::sync::Once::new();
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    INIT.call_once(|| {
        autoreleasepool(|_| {
            let center = NSNotificationCenter::defaultCenter();
            let observer = ReopenObserver::new(mtm);

            // SAFETY: `observer` responds to `onNotification:` (defined above), stays retained
            // in `REOPEN_OBSERVER` until `teardown_reopen_observer` removes it, and both
            // notification name constants are valid static `NSNotificationName` globals.
            unsafe {
                // Observe NSApplicationDidBecomeActiveNotification (fires on Dock click, Cmd+Tab, app activate)
                center.addObserver_selector_name_object(
                    &observer,
                    sel!(onNotification:),
                    Some(NSApplicationDidBecomeActiveNotification),
                    None,
                );
                // Observe NSApplicationDidUnhideNotification (fires when macOS unhides on Dock click / Cmd+Tab)
                center.addObserver_selector_name_object(
                    &observer,
                    sel!(onNotification:),
                    Some(NSApplicationDidUnhideNotification),
                    None,
                );
            }

            REOPEN_OBSERVER.with(|cell| {
                *cell.borrow_mut() = Some(observer);
            });
        });
    });
}

pub(super) fn teardown_reopen_observer() {
    if MainThreadMarker::new().is_none() {
        return;
    }
    let observer = REOPEN_OBSERVER.with(|cell| cell.borrow_mut().take());
    if let Some(observer) = observer {
        autoreleasepool(|_| {
            let center = NSNotificationCenter::defaultCenter();
            // SAFETY: `observer` was registered with `center` in `setup_reopen_observer`
            // and is still alive until dropped at the end of this block.
            unsafe {
                center.removeObserver(&observer);
            }
        });
    }
    // Release the cloned egui::Context held in the global static on shutdown.
    let mut lock = lock_or_recover(&REPAINT_CTX, "teardown_reopen_observer");
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
    autoreleasepool(|_| {
        let key = ns_string!("AXTrustedCheckOptionPrompt");
        let val = NSNumber::numberWithBool(true);
        let dict = NSDictionary::from_slices(&[key], &[&*val]);
        // SAFETY: `dict` is a valid, toll-free-bridged `CFDictionary` kept alive for the call.
        unsafe { AXIsProcessTrustedWithOptions(Retained::as_ptr(&dict).cast::<std::ffi::c_void>()) }
    })
}

pub(crate) fn open_accessibility_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

pub(super) fn capture_frontmost() {
    autoreleasepool(|_| {
        let workspace = NSWorkspace::sharedWorkspace();
        let Some(frontmost) = workspace.frontmostApplication() else {
            return;
        };
        let pid = frontmost.processIdentifier();
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
    autoreleasepool(|_| {
        let pid = PREVIOUS_APP_PID.load(Ordering::Relaxed);
        if pid > 0 {
            if let Some(target_app) =
                NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
            {
                #[allow(deprecated)]
                let _ = target_app
                    .activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
            }
        }
    });
}

pub(super) fn is_target_foreground() -> bool {
    let target_pid = PREVIOUS_APP_PID.load(Ordering::Relaxed);
    if target_pid <= 0 {
        return false;
    }
    autoreleasepool(|_| {
        let workspace = NSWorkspace::sharedWorkspace();
        let Some(frontmost) = workspace.frontmostApplication() else {
            return false;
        };
        frontmost.processIdentifier() == target_pid
    })
}

pub(super) fn simulate_paste() {
    // macOS ANSI virtual keycode for 'v' is 9
    const KEY_V: u16 = 9;
    // kCGEventFlagMaskCommand = 0x0010_0000
    const MASK_COMMAND: u64 = 0x0010_0000;
    // kCGHIDEventTap = 0 (flows down into session/app; posting to both HID and SESSION double-pastes)
    const TAP_LOCATION_HID: u32 = 0;
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
            CFRelease(event_down);
        }

        // Brief delay between key down and key up to ensure runloop registration
        std::thread::sleep(std::time::Duration::from_millis(15));

        // Release Command+V
        let event_up = CGEventCreateKeyboardEvent(source, KEY_V, false);
        if !event_up.is_null() {
            CGEventSetFlags(event_up, MASK_COMMAND);
            CGEventPost(TAP_LOCATION_HID, event_up);
            CFRelease(event_up);
        }

        if !source.is_null() {
            CFRelease(source);
        }
    }
}

/// On macOS, `CGEventSetFlags(event, MASK_COMMAND)` explicitly overrides physical modifier state,
/// so waiting for physical modifier release is unnecessary and would stall rapid `Cmd+Option+V` presses.
pub(super) const fn are_modifiers_held(_modifiers: global_hotkey::hotkey::Modifiers) -> bool {
    false
}

pub(super) fn simulate_paste_foreground(
    _release_modifiers: global_hotkey::hotkey::Modifiers,
) -> Result<(), &'static str> {
    if !is_trusted() {
        return Err("Accessibility permission missing");
    }
    simulate_paste();
    Ok(())
}

fn general_pasteboard() -> Option<Retained<NSPasteboard>> {
    // SAFETY: `+[NSPasteboard generalPasteboard]` is a standard AppKit class method.
    // Receiving `Option<Retained<_>>` avoids panicking when the pasteboard server is
    // unavailable (e.g. headless/sandboxed test runners or early login sessions).
    unsafe { msg_send![NSPasteboard::class(), generalPasteboard] }
}

pub(super) fn clipboard_change_token() -> Option<u64> {
    autoreleasepool(|_| {
        let pasteboard = general_pasteboard()?;
        let count = pasteboard.changeCount();
        Some((count as i64).cast_unsigned())
    })
}

pub(super) fn is_clipboard_concealed() -> bool {
    autoreleasepool(|_| {
        let Some(pasteboard) = general_pasteboard() else {
            return false;
        };
        let Some(types) = pasteboard.types() else {
            return false;
        };
        let markers = [
            ns_string!("org.nspasteboard.ConcealedType"),
            ns_string!("org.nspasteboard.AutoGeneratedType"),
            ns_string!("org.nspasteboard.TransientType"),
            ns_string!("com.agilebits.onepassword"),
            ns_string!("de.petermaurer.TransientPasteboardType"),
        ];
        for item_type in types {
            if markers.iter().any(|&marker| &*item_type == marker) {
                return true;
            }
        }
        false
    })
}

pub(super) fn mark_clipboard_concealed() {
    autoreleasepool(|_| {
        let Some(pasteboard) = general_pasteboard() else {
            return;
        };
        let empty_data = NSData::new();
        for marker in [
            ns_string!("org.nspasteboard.ConcealedType"),
            ns_string!("org.nspasteboard.TransientType"),
        ] {
            let types = NSArray::from_slice(&[marker]);
            // SAFETY: `types` is a valid non-empty `NSArray<NSPasteboardType>` and owner is `None`.
            unsafe {
                let _ = pasteboard.addTypes_owner(&types, None);
            }
            let _ = pasteboard.setData_forType(Some(&empty_data), marker);
        }
    });
}

#[link(name = "Vision", kind = "framework")]
extern "C" {}

pub(super) fn supports_image_ocr() -> bool {
    // Apple Vision framework is available on macOS 10.15+.
    // Verify required classes are registered in the ObjC runtime.
    AnyClass::get(c"VNImageRequestHandler").is_some()
        && AnyClass::get(c"VNRecognizeTextRequest").is_some()
}

/// Looks up the two Vision classes needed for OCR, keeping the original
/// class-not-found error messages. This only queries the `ObjC` runtime and
/// performs no message sends, so it needs no autorelease pool of its own.
fn ocr_classes() -> Result<(&'static AnyClass, &'static AnyClass), String> {
    let handler_cls = AnyClass::get(c"VNImageRequestHandler")
        .ok_or_else(|| "VNImageRequestHandler class not found".to_string())?;
    let request_cls = AnyClass::get(c"VNRecognizeTextRequest")
        .ok_or_else(|| "VNRecognizeTextRequest class not found".to_string())?;
    Ok((handler_cls, request_cls))
}

/// # Safety
/// All `ObjC` message sends happen inside the caller's autorelease pool, and
/// `handler_cls` / `request_cls` must be `VNImageRequestHandler` and
/// `VNRecognizeTextRequest`.
unsafe fn build_request_handler(
    png_bytes: &[u8],
    handler_cls: &'static AnyClass,
    request_cls: &'static AnyClass,
) -> Result<(Retained<AnyObject>, Retained<AnyObject>), String> {
    let ns_data = NSData::with_bytes(png_bytes);

    let handler_alloc: Allocated<AnyObject> = msg_send![handler_cls, alloc];
    let handler: Option<Retained<AnyObject>> = msg_send![
        handler_alloc,
        initWithData: &*ns_data,
        options: std::ptr::null::<AnyObject>()
    ];
    let handler = handler.ok_or_else(|| "Failed to create VNImageRequestHandler".to_string())?;

    let request_alloc: Allocated<AnyObject> = msg_send![request_cls, alloc];
    let request: Option<Retained<AnyObject>> = msg_send![request_alloc, init];
    let request = request.ok_or_else(|| "Failed to create VNRecognizeTextRequest".to_string())?;

    // VNRequestTextRecognitionLevelAccurate = 1
    let _: () = msg_send![&*request, setRecognitionLevel: 1_isize];
    let _: () = msg_send![&*request, setUsesLanguageCorrection: true];

    Ok((handler, request))
}

/// # Safety
/// `handler` must be a `VNImageRequestHandler` and `request` a `VNRecognizeTextRequest`,
/// called inside an active autorelease pool.
unsafe fn perform_ocr_request(handler: &AnyObject, request: &AnyObject) -> Result<String, String> {
    let req_array = NSArray::from_slice(&[request]);
    let res: Result<(), Retained<NSError>> =
        msg_send![handler, performRequests: &*req_array, error: _];
    match res {
        Ok(()) => Ok(recognized_text(request)),
        Err(err) => {
            let msg = err.localizedDescription().to_string();
            if msg.is_empty() {
                Err("Vision text recognition request failed".to_string())
            } else {
                Err(msg)
            }
        }
    }
}

/// Returns the recognized string of an observation's top candidate.
///
/// # Safety
/// `obs` must be a `VNRecognizedTextObservation` inside an active autorelease pool.
unsafe fn first_candidate_text(obs: &AnyObject) -> Option<String> {
    let top_candidates: Option<Retained<NSArray<AnyObject>>> =
        msg_send![obs, topCandidates: 1_usize];
    let cand = top_candidates?.firstObject()?;
    let str_obj: Option<Retained<NSString>> = msg_send![&*cand, string];
    str_obj.map(|s| s.to_string())
}

/// # Safety
/// `request` must be a `VNRecognizeTextRequest` whose results have been populated
/// inside an active autorelease pool.
unsafe fn recognized_text(request: &AnyObject) -> String {
    let results: Option<Retained<NSArray<AnyObject>>> = msg_send![request, results];
    let Some(results) = results else {
        return String::new();
    };
    let mut lines = Vec::with_capacity(results.len());
    for obs in results {
        if let Some(line) = first_candidate_text(&obs) {
            lines.push(line);
        }
    }
    lines.join("\n")
}

pub(super) fn extract_text_from_png(png_bytes: &[u8]) -> Result<String, String> {
    if !supports_image_ocr() {
        return Err("Apple Vision framework is not available on this macOS version".to_string());
    }
    if png_bytes.is_empty() {
        return Err("Image payload is empty".to_string());
    }

    autoreleasepool(|_| {
        let (handler_cls, request_cls) = ocr_classes()?;
        // SAFETY: Both classes were validated by `ocr_classes()`, all Vision message sends
        // run inside this autorelease pool, and `Retained` releases `handler`/`request` on drop.
        unsafe {
            let (handler, request) = build_request_handler(png_bytes, handler_cls, request_cls)?;
            perform_ocr_request(&handler, &request)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_or_recover_recovers_poisoned_mutex() {
        let mutex = Mutex::new(0_u8);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = mutex.lock().unwrap();
            panic!("intentional poison for poison-recovery test");
        }));
        assert!(result.is_err());
        assert!(mutex.is_poisoned());

        *lock_or_recover(&mutex, "test") = 42;
        assert_eq!(*lock_or_recover(&mutex, "test-assert"), 42);
    }
}
