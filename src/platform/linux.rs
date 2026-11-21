use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConnectionExt, CreateWindowAux, EventMask, InputFocus, Window,
    WindowClass, CLIENT_MESSAGE_EVENT,
};
use x11rb::protocol::xtest::ConnectionExt as XTestConnectionExt;
use x11rb::rust_connection::RustConnection;

static PREVIOUS_X11_WINDOW: AtomicU32 = AtomicU32::new(0);

const X11_RECONNECT_BACKOFF: Duration = Duration::from_secs(5);

struct X11Session {
    conn: RustConnection,
    root: Window,
    net_active_window: u32,
    net_wm_pid: u32,
    clipboard: u32,
    targets: u32,
    password_manager_hint: u32,
    cipherclipboard_targets: u32,
}

impl X11Session {
    fn connect() -> Option<Self> {
        let (conn, screen_num) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots.get(screen_num)?.root;

        // Pipeline all atom interning requests in a single flush/round-trip batch.
        let c_active = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW").ok()?;
        let c_pid = conn.intern_atom(true, b"_NET_WM_PID").ok()?;
        let c_clipboard = conn.intern_atom(false, b"CLIPBOARD").ok()?;
        let c_targets = conn.intern_atom(false, b"TARGETS").ok()?;
        let c_hint = conn.intern_atom(false, b"x-kde-passwordManagerHint").ok()?;
        let c_prop = conn.intern_atom(false, b"_CIPHERCLIPBOARD_TARGETS").ok()?;

        let net_active_window = c_active.reply().ok().map_or(0, |r| r.atom);
        let net_wm_pid = c_pid.reply().ok().map_or(0, |r| r.atom);
        let clipboard = c_clipboard.reply().ok().map_or(0, |r| r.atom);
        let targets = c_targets.reply().ok().map_or(0, |r| r.atom);
        let password_manager_hint = c_hint.reply().ok().map_or(0, |r| r.atom);
        let cipherclipboard_targets = c_prop.reply().ok().map_or(0, |r| r.atom);

        Some(Self {
            conn,
            root,
            net_active_window,
            net_wm_pid,
            clipboard,
            targets,
            password_manager_hint,
            cipherclipboard_targets,
        })
    }
}

enum X11State {
    Uninitialized,
    Connected(Box<X11Session>),
    Unavailable(Instant),
}

thread_local! {
    static X11_STATE: RefCell<X11State> = const { RefCell::new(X11State::Uninitialized) };
}

/// Runs `f` with this thread's persistent X11 connection and cached atoms.
/// Avoids reconnecting on every UI frame or 200ms monitor tick, and backs off
/// when X11 is unavailable (e.g. pure Wayland).
fn with_x11<T>(f: impl FnOnce(&X11Session) -> Option<T>) -> Option<T> {
    X11_STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        let needs_connect = match &*state {
            X11State::Uninitialized => true,
            X11State::Unavailable(next_retry) => Instant::now() >= *next_retry,
            X11State::Connected(_) => false,
        };
        if needs_connect {
            *state = X11Session::connect().map_or_else(
                || X11State::Unavailable(Instant::now() + X11_RECONNECT_BACKOFF),
                |s| X11State::Connected(Box::new(s)),
            );
        }
        let X11State::Connected(session) = &*state else {
            return None;
        };
        let result = f(session);
        // If the underlying X11 connection died, drop it so the next call reconnects.
        if result.is_none() && session.conn.flush().is_err() {
            *state = X11State::Uninitialized;
        }
        result
    })
}

fn active_window_and_pid() -> Option<(Window, Option<u32>)> {
    with_x11(|s| {
        if s.net_active_window == 0 {
            return None;
        }

        let active_reply = s
            .conn
            .get_property(false, s.root, s.net_active_window, AtomEnum::WINDOW, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        let win = active_reply.value32()?.next()?;
        if win == 0 {
            return None;
        }

        let pid = if s.net_wm_pid == 0 {
            None
        } else {
            s.conn
                .get_property(false, win, s.net_wm_pid, AtomEnum::CARDINAL, 0, 1)
                .ok()
                .and_then(|c| c.reply().ok())
                .and_then(|r| r.value32()?.next())
        };

        Some((win, pid))
    })
}

pub(super) fn is_active() -> Option<bool> {
    let (_win, pid) = active_window_and_pid()?;
    pid.map(|p| p == std::process::id())
}

pub(super) fn capture_frontmost() {
    if let Some((win, pid)) = active_window_and_pid() {
        if pid != Some(std::process::id()) {
            PREVIOUS_X11_WINDOW.store(win, Ordering::Relaxed);
        }
    }
}

pub(super) fn restore_focus() {
    let win = PREVIOUS_X11_WINDOW.load(Ordering::Relaxed);
    if win == 0 {
        return;
    }

    let _ = with_x11(|s| {
        if s.net_active_window != 0 {
            let event = ClientMessageEvent {
                response_type: CLIENT_MESSAGE_EVENT,
                format: 32,
                sequence: 0,
                window: win,
                type_: s.net_active_window,
                data: [1, x11rb::CURRENT_TIME, 0, 0, 0].into(),
            };
            let _ = s.conn.send_event(
                false,
                s.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            );
        }

        let _ = s
            .conn
            .set_input_focus(InputFocus::PARENT, win, x11rb::CURRENT_TIME);
        let _ = s.conn.flush();
        Some(())
    });
}

pub(super) fn is_target_foreground() -> bool {
    let target_win = PREVIOUS_X11_WINDOW.load(Ordering::Relaxed);
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

    let _ = with_x11(|s| {
        let ctrl_kc = find_keycode(&s.conn, XK_CONTROL_L)?;
        let v_kc = find_keycode(&s.conn, XK_V_LOWER)?;

        let _ = s
            .conn
            .xtest_fake_input(KEY_PRESS, ctrl_kc, 0, s.root, 0, 0, 0);
        let _ = s.conn.xtest_fake_input(KEY_PRESS, v_kc, 0, s.root, 0, 0, 0);
        let _ = s.conn.flush();

        std::thread::sleep(Duration::from_millis(15));

        let _ = s
            .conn
            .xtest_fake_input(KEY_RELEASE, v_kc, 0, s.root, 0, 0, 0);
        let _ = s
            .conn
            .xtest_fake_input(KEY_RELEASE, ctrl_kc, 0, s.root, 0, 0, 0);
        let _ = s.conn.flush();
        Some(())
    });
}

/// Marker emission is intentionally unavailable on Linux/X11.
///
/// `arboard` owns the `CLIPBOARD` selection when a secret is copied, so adding
/// `x-kde-passwordManagerHint` would require taking ownership and proxying every
/// requested target. Detection remains available via [`is_clipboard_concealed`],
/// and the UI discloses this limitation.
pub(super) fn mark_clipboard_concealed() {
    #[cfg(test)]
    MARK_CALLED.store(true, Ordering::Release);
}

#[cfg(test)]
static MARK_CALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
pub(super) fn reset_mark_called() {
    MARK_CALLED.store(false, Ordering::Release);
}

#[cfg(test)]
pub(super) fn mark_called() -> bool {
    MARK_CALLED.load(Ordering::Acquire)
}

/// Detects concealed clipboard entries on X11 (e.g. `KeePassXC`, Bitwarden, 1Password)
/// by inspecting the `CLIPBOARD` selection's advertised `TARGETS` for
/// `x-kde-passwordManagerHint`.
pub(super) fn is_clipboard_concealed() -> bool {
    with_x11(|s| {
        if s.clipboard == 0
            || s.targets == 0
            || s.password_manager_hint == 0
            || s.cipherclipboard_targets == 0
        {
            return Some(false);
        }

        // Fast path: if nobody owns CLIPBOARD, there's nothing concealed.
        let owner_reply = s.conn.get_selection_owner(s.clipboard).ok()?.reply().ok()?;
        if owner_reply.owner == 0 {
            return Some(false);
        }

        let win = s.conn.generate_id().ok()?;
        if s.conn
            .create_window(
                0,
                win,
                s.root,
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
            return Some(false);
        }

        let concealed = if s
            .conn
            .convert_selection(
                win,
                s.clipboard,
                s.targets,
                s.cipherclipboard_targets,
                x11rb::CURRENT_TIME,
            )
            .is_ok()
            && s.conn.flush().is_ok()
        {
            poll_selection_for_hint(&s.conn, win, s.password_manager_hint)
        } else {
            false
        };

        let _ = s.conn.destroy_window(win);
        let _ = s.conn.flush();
        Some(concealed)
    })
    .unwrap_or(false)
}

fn poll_selection_for_hint(conn: &RustConnection, win: u32, hint_atom: u32) -> bool {
    use x11rb::protocol::Event;

    let deadline = Instant::now() + Duration::from_millis(50);
    while Instant::now() < deadline {
        match conn.poll_for_event() {
            Ok(Some(Event::SelectionNotify(ev))) => {
                if ev.requestor != win {
                    continue;
                }
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
