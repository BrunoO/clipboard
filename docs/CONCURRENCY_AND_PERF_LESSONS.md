# Concurrency & Performance Lessons Learned

Distilled from the CipherClipboard multithreading, concurrency, and `eframe` 0.36 performance reviews. Future contributors and coding agents should treat these as concrete invariants when modifying threads, synchronization, platform polling, database access, or the UI event loop.

---

## 1. Thread Lifecycle, Naming, & Clean Shutdown

- **Always handle channel `Disconnected` alongside explicit shutdown signals.** When a background loop waits on `recv_timeout` or `try_recv`, treating `Disconnected` as a no-op causes 100% CPU busy-spins (e.g., `init_clipboard_with_retry`) or leaked threads (e.g., Linux `QuoteFallback::run_loop`). Always exit and release OS resources (like X11 key grabs) on `Ok(Shutdown) | Err(Disconnected)`.
- **Name every spawned thread and join owned workers on shutdown.** Use `std::thread::Builder::new().name(...)` (`clipboard-monitor`, `hotkey-listener`, `x11-quote-hotkey`, `focus-paste-worker`) so the panic hook and profilers never report `<unnamed>`. Store `JoinHandle`s and `.join()` them in `Drop`/`shutdown()` so in-flight SQLite WAL writes and X11 ungrabs complete deterministically before exit.
- **Serialize OS side effects with a single worker instead of detached per-action threads.** Spawning a detached thread per paste (`restore_focus_and_paste`) lets rapid triggers race on focus restoration and interleave synthetic key-down/key-up events. Use one persistent worker thread (`focus-paste-worker`) with `recv_timeout` during the paste delay so newer requests cleanly supersede stale ones.

---

## 2. Locks, Shared State, & Memory Ordering

- **Never split coupled state across independent atomics.** Storing Windows `HWND` and `PID` in separate atomics (`PREVIOUS_HWND` + `PREVIOUS_PID`) permits torn reads and lets a stale-window clear clobber a freshly captured window. Keep coupled fields in a single `Mutex<Option<T>>` and use conditional clearing (`clear_previous_window_if_matches`).
- **Keep `Mutex` critical sections minimal and free of CPU-bound work.**
  - Classify clipboard text (`is_likely_secret`, URL/color/code detection) *before* acquiring the SQLite `Mutex` (`Database::insert`), and classify legacy rows *after* dropping it (`Database::get_recent`).
  - Wrap multi-statement writes (duplicate check → delete stale copy → insert → prune) in a single SQLite `conn.transaction()?` so they commit atomically in one WAL write and avoid redundant UI-thread `prune_history()` calls.
- **Synchronize shared file I/O and log rotation safely across panic boundaries.** Guard `app.log` size-check + rotation + append with `LOG_LOCK: Mutex<()>` (caching the log directory in `OnceLock`), and use `try_lock()` when `std::thread::panicking()` so the panic hook can never self-deadlock.
- **Don't use `Arc<RwLock<T>>` or `Mutex<T>` where ownership is single-threaded.**
  - UI-only fields like `HotkeyService::last_registration_results` should be plain `Vec<T>`, not `Arc<RwLock<Vec<T>>>`.
  - Strictly main-thread-only FFI handles (macOS `REOPEN_OBSERVER`, guarded by `is_main_thread()`) belong in `thread_local! { Cell<*mut Object> }` rather than `unsafe impl Send/Sync` + `Mutex`.
- **Consolidate shared state into one `Arc<Inner>`.** Group shared handles (`DatabaseInner`) behind a single `Arc` instead of cloning 5 separate `Arc` fields per handle.
- **Choose appropriate atomic orderings and poison recovery.** Use `Relaxed` for standalone IDs (`PREVIOUS_APP_PID`, `PREVIOUS_X11_WINDOW`, cached Win32 format IDs) and `Release`/`AcqRel` for signal flags (`REOPEN_REQUESTED`). Recover from poisoned mutexes consistently — including in test mocks (`mock_keyring`) so one failing test never cascades across parallel `cargo test` runs.
- **Always check Win32 `AttachThreadInput` return values** before calling the matching detach (`AttachThreadInput(..., 0)`).

---

## 3. Event-Driven Blocking & Bounded Wakeups

- **Eliminate idle polling loops whenever a blocking primitive exists:**
  - Use `crossbeam_channel::select!` in `HotkeyService` over `GlobalHotKeyEvent::receiver()` and a `crossbeam_channel::bounded(1)` shutdown channel (0 idle wakeups).
  - In Linux `QuoteFallback::run_loop`, block on `rx.recv()` when `grabs.is_empty()` and only use `rx.recv_timeout(50ms)` when a fallback grab is actually active.
- **Use bounded `sync_channel(1)` + `try_send(())` for coalesced notifications.** Channels whose receiver drains all pending messages into a single boolean (`notify_tx`, `hotkey_tx`) should be bounded to capacity 1 so slow/blocked consumers never accumulate unbounded messages.

---

## 4. Platform Polling & OS Resource Efficiency

- **Gate clipboard reads on OS change counters before copying payloads.** Reading text/images every 200ms copies multi-megabyte RGBA buffers repeatedly while idle. Check `NSPasteboard.changeCount` (macOS) and `GetClipboardSequenceNumber()` (Windows) first, and only commit the new `change_token` when `arboard` does not return `Error::ClipboardOccupied`.
- **Avoid redundant fallback reads on unchanged text.** In `poll_clipboard_text`, return `PollOutcome::Handled` (not `NotAvailable`) when text matches `last_copied_text` or exceeds `MAX_TEXT_LENGTH`, so platforms without a change counter (Linux) don't fall through to `get_image()` every 200ms.
- **Reuse persistent X11 connections and interned atoms per thread on Linux.** Never call `x11rb::connect(None)` or `intern_atom` on every UI frame or monitor tick. Keep a thread-local `X11Session` (zero cross-thread lock contention and isolated event queues) with pipelined atom interning, automatic reconnect on broken connections, a 5-second backoff on pure Wayland, and `ev.requestor != win` filtering for `SelectionNotify`.
- **Cache Win32 registered clipboard formats** (`cached_clipboard_format` with `AtomicU32`) on both read (`is_clipboard_concealed`) and write (`mark_clipboard_concealed`) paths.

---

## 5. `eframe` 0.36+ Lifecycle & UI Thread Performance

- **Implement `eframe::App::logic()` for all hidden-window and background-event handling.** In `eframe` 0.36+, `App::ui()` is **not called at all** while the root viewport is hidden (`Visible(false)`). Put hotkey, tray, activation/reopen, close-request (`CancelClose`), clipboard monitor, and secret auto-clear handlers in `logic()`, which runs before every `ui()` frame *and* via `update_logic_only` while hidden.
- **Never read stale `ctx.input()` events inside `App::logic()` while hidden.** During `update_logic_only`, `i.viewport()` is fresh, but `i.events`, `i.key_pressed`, and `i.pointer` remain from the last visible frame. Keep keyboard/UI shortcuts (`Escape`, `Cmd/Ctrl+Q`, list navigation) in `App::ui()`.
- **Do not add unconditional background repaint polling timers in `eframe` 0.36+.** `eframe` 0.36 natively wakes invisible/minimized windows on `ctx.request_repaint()` from background threads (throttled internally to `INVISIBLE_WINDOW_REPAINT_INTERVAL = 100ms`). Use a one-shot exact-deadline `ctx.request_repaint_after(deadline.saturating_duration_since(Instant::now()))` for timed actions like secret auto-clear, and let the event loop sleep at 0% idle CPU otherwise.
- **Precompute per-list UI aggregates on data refresh, not per frame.** Compute filter tab counts (`FilterCounts::from_items`) in a single pass inside `refresh_items()` instead of iterating `self.items` multiple times and running `embedded_colors()` on every visible frame.
